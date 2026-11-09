//! Starting, waking and *Lock now* lock first, whatever else fails: the kind
//! put back with the code is asked for in the same callback but takes effect
//! after it, and neither it nor the tries nor the screen's refusal may leave
//! the device open.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use core::cell::{Cell, RefCell};
use paperpaw_plugin_lock::native_flow::{Context, Effects, Flow, Lockstate};
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
    /// Commands staged in the callback running, and those of callbacks that
    /// returned `Ok`: an error returned drops what the callback staged.
    staged: RefCell<Vec<&'static str>>,
    applied: RefCell<Vec<&'static str>>,
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
            staged: RefCell::new(Vec::new()),
            applied: RefCell::new(Vec::new()),
        }
    }

    /// One callback: what it returns decides whether its commands apply.
    fn callback(&self, run: impl FnOnce() -> Result<(), Error>) -> Result<(), Error> {
        let result = run();
        let staged = self.staged.take();
        if result.is_ok() {
            self.applied.borrow_mut().extend(staged);
        }
        result
    }

    /// How many times the lock screen was opened by callbacks that returned.
    fn opened(&self) -> usize {
        self.applied
            .borrow()
            .iter()
            .filter(|c| **c == "open")
            .count()
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
        if let Some(error) = self.repair_error {
            return Err(error);
        }
        self.staged.borrow_mut().push("kind");
        Ok(())
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
        self.staged.borrow_mut().push("open");
        Ok(())
    }
}

fn fresh() -> (Lockstate, Door) {
    (Lockstate::default(), Door::default())
}

fn start(device: &Fake, lock: &mut Lockstate, door: &mut Door, now_ms: u64) -> Result<(), Error> {
    device.callback(|| gate::started(device, lock, door, now_ms))
}

fn wake(device: &Fake, lock: &mut Lockstate, door: &mut Door) -> Result<(), Error> {
    device.callback(|| gate::woke(device, lock, door))
}

fn now(device: &Fake, lock: &mut Lockstate, door: &mut Door) -> Result<(), Error> {
    device.callback(|| gate::lock_now(device, lock, door))
}

fn reopened(device: &Fake, lock: &mut Lockstate, door: &mut Door) -> Result<bool, Error> {
    let mut asked = false;
    device.callback(|| {
        asked = gate::reopen(device, lock, door)?;
        Ok(())
    })?;
    Ok(asked)
}

#[test]
fn a_code_kept_locks_at_the_start_whatever_the_kind_setting_says() {
    // The kind setting says `off` (a reset): the repair is queued, but a
    // command takes effect after the callback, so the lock must not wait for
    // it.
    let device = Fake::with_code("pin");
    let (mut lock, mut door) = fresh();
    start(&device, &mut lock, &mut door, 0).unwrap();
    assert!(lock.locked);
    assert_eq!(device.opened(), 1);
    assert_eq!(device.repaired.borrow().len(), 1);
}

#[test]
fn no_code_locks_nothing() {
    let device = Fake::none();
    let (mut lock, mut door) = fresh();
    start(&device, &mut lock, &mut door, 0).unwrap();
    wake(&device, &mut lock, &mut door).unwrap();
    now(&device, &mut lock, &mut door).unwrap();
    assert!(!lock.locked);
    assert_eq!(device.opened(), 0);
}

#[test]
fn a_damaged_code_locks_nothing() {
    let mut device = Fake::none();
    device.stored = Some("pin::".to_owned());
    let (mut lock, mut door) = fresh();
    start(&device, &mut lock, &mut door, 0).unwrap();
    assert!(!lock.locked);
}

#[test]
fn the_start_still_opens_the_screen_when_the_repair_and_the_tries_fail() {
    // An error returned drops what the callback staged: the callback returns
    // `Ok` once the screen is staged, and the rest is left for the next time.
    let mut device = Fake::with_code("pattern");
    device.repair_error = Some(Error::NoMemory);
    device.tries = Err(Error::Io);
    let (mut lock, mut door) = fresh();
    assert_eq!(start(&device, &mut lock, &mut door, 0), Ok(()));
    assert!(lock.locked);
    assert_eq!(device.opened(), 1);
}

#[test]
fn the_tries_are_kept_when_only_the_repair_fails() {
    let mut device = Fake::with_code("pin");
    device.repair_error = Some(Error::Io);
    device.tries = Ok(5);
    let (mut lock, mut door) = fresh();
    start(&device, &mut lock, &mut door, 1_000).unwrap();
    assert!(lock.locked);
    assert_eq!(device.opened(), 1);
    assert!(lock.attempts.waiting(1_001));
}

#[test]
fn a_code_that_cannot_be_read_is_reported_and_the_rest_is_done() {
    let mut device = Fake::with_code("pin");
    device.stored_error.set(Some(Error::Io));
    device.tries = Ok(5);
    let (mut lock, mut door) = fresh();
    assert_eq!(start(&device, &mut lock, &mut door, 0), Err(Error::Io));
    assert!(lock.attempts.waiting(1));
    assert!(device.repaired.borrow().is_empty());
}

#[test]
fn a_refused_screen_leaves_the_device_locked_and_is_reported() {
    let device = Fake::with_code("pin");
    device.open_refusals.set(1);
    let (mut lock, mut door) = fresh();
    assert_eq!(start(&device, &mut lock, &mut door, 0), Err(Error::Busy));
    assert!(lock.locked);
    assert!(!door.opening);
    assert_eq!(device.opened(), 0);
    // Locked and no screen: the next occasion opens it.
    now(&device, &mut lock, &mut door).unwrap();
    assert_eq!(device.opened(), 1);
    assert!(door.opening);
}

#[test]
fn a_screen_that_was_closed_and_not_reopened_is_opened_by_lock_now() {
    let device = Fake::with_code("password");
    let (mut lock, mut door) = fresh();
    now(&device, &mut lock, &mut door).unwrap();
    // It showed, then something closed it, and reopening it was refused.
    door.opening = false;
    door.shown = false;
    device.open_refusals.set(1);
    assert_eq!(reopened(&device, &mut lock, &mut door), Err(Error::Busy));
    assert!(lock.locked);
    now(&device, &mut lock, &mut door).unwrap();
    assert_eq!(device.opened(), 2);
}

#[test]
fn a_stale_opening_does_not_keep_lock_now_from_opening_the_screen() {
    // The screen was staged in a callback that then failed (its commands were
    // dropped) or the host refused it: `opening` is stale, no screen is there.
    let device = Fake::with_code("pin");
    let (mut lock, mut door) = fresh();
    lock.locked = true;
    door.opening = true;
    now(&device, &mut lock, &mut door).unwrap();
    assert_eq!(device.opened(), 1);
}

#[test]
fn a_screen_that_is_there_or_on_its_way_is_not_opened_twice() {
    let device = Fake::with_code("pin");
    let (mut lock, mut door) = fresh();
    now(&device, &mut lock, &mut door).unwrap();
    // On its way, in the same callback or the next that comes before it shows.
    assert!(!reopened(&device, &mut lock, &mut door).unwrap());
    assert_eq!(device.opened(), 1);
    door.opening = false;
    door.shown = true;
    now(&device, &mut lock, &mut door).unwrap();
    assert!(!reopened(&device, &mut lock, &mut door).unwrap());
    assert_eq!(device.opened(), 1);
}

#[test]
fn waking_locks_unless_it_is_set_not_to_and_where_that_cannot_be_read_it_does() {
    let mut device = Fake::with_code("pin");
    device.wake = Ok(false);
    let (mut lock, mut door) = fresh();
    wake(&device, &mut lock, &mut door).unwrap();
    assert!(!lock.locked);

    device.wake = Err(Error::Io);
    device.repair_error = Some(Error::NoMemory);
    let (mut lock, mut door) = fresh();
    // Locked and the screen staged: nothing is reported that would drop it.
    wake(&device, &mut lock, &mut door).unwrap();
    assert!(lock.locked);
    assert_eq!(device.opened(), 1);
}

#[test]
fn waking_opens_the_screen_before_the_repair_and_it_failing_changes_nothing() {
    let mut device = Fake::with_code("password");
    device.repair_error = Some(Error::NoMemory);
    let (mut lock, mut door) = fresh();
    assert_eq!(wake(&device, &mut lock, &mut door), Ok(()));
    assert!(lock.locked);
    assert_eq!(device.opened(), 1);
}

#[test]
fn the_kind_setting_never_decides_which_lock_is_in_use() {
    // `Lock now` with a code kept and the kind setting `off`: the first key
    // must not unlock without the code.
    let device = Fake::with_code("pin");
    let stored = device.stored.clone();
    let (mut lock, mut door) = fresh();
    now(&device, &mut lock, &mut door).unwrap();
    assert!(lock.locked);
    let active = gate::active(stored.as_deref());
    assert_eq!(active, Some("pin"));
    let ctx = Context {
        stored: stored.as_deref(),
        active,
        now_ms: 0,
        touch: true,
    };
    let mut flow = Flow::new(true, ctx.active, false);
    let mut fx = Effects::default();
    // Nothing typed, *OK*: no code, no unlock; and a screen shown again does
    // not release the device either.
    let ok = model::KEYS.iter().position(|k| *k == "OK").unwrap();
    flow.press(ok, &mut lock, &ctx, &mut fx).unwrap();
    assert!(lock.locked);
    assert!(!flow.released(&mut lock, &ctx, &mut fx));
    assert!(lock.locked);
    // The code removed elsewhere is what releases it.
    let gone = Context {
        stored: None,
        active: gate::active(None),
        ..ctx
    };
    assert!(flow.released(&mut lock, &gone, &mut fx));
}
