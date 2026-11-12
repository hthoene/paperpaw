//! The inside of a [`NativeLease`](super::native_instances::NativeLease): the
//! plugin's live instance, or, for a parked plugin, the recipe that makes it
//! again (#248, `docs/measurements/parked-plugins.md`).
//!
//! A parked plugin is running for every plugin and for itself: the one
//! accessor, [`Slot::live`], makes it again at a safe point (no callback
//! running, no event batch being delivered, no registration transaction open)
//! and answers `ERR_BUSY` anywhere else, the plugin asked to start on the next
//! pass. It never answers "stopped" for a parked plugin. The slot changes once,
//! parked to live; it never goes back (phase 1).
use super::{native_callbacks::CallbackRuntime, native_instances::NativeInstance};
use alloc::boxed::Box;
use core::cell::{Cell, OnceCell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_CANCELLED};

/// The first wait after an unpark found no memory, and the longest.
const RETRY_FIRST_MS: u64 = 1_000;
const RETRY_LAST_MS: u64 = 60_000;

/// Makes a parked plugin's instance again: re-hosted under the same owner id,
/// `make` run once more, its registration batch checked against the one of
/// the start.
pub(crate) trait Unpark {
    /// The instance, made anew.
    ///
    /// # Errors
    /// [`Unparked::NoMemory`] where the heap has no room (the plugin stays
    /// parked); [`Unparked::Refused`] where `make` failed or its batch is not
    /// the one of the start (the plugin is withdrawn).
    fn unpark(&self) -> Result<Box<dyn NativeInstance>, Unparked>;
    /// Whether the plugin subscribed to the event `tag` (kept from the start).
    fn subscribed(&self, tag: u32) -> bool;
    /// The plugin's image (a linked plugin's never moves).
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>));
}

/// Why an unpark made no instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unparked {
    NoMemory,
    Refused,
}

/// A parked plugin's part of the slot.
struct Parked {
    recipe: Box<dyn Unpark>,
    clock: CallbackRuntime,
    /// Asked to start where it could not be made (not a safe point, or a
    /// retry waiting): made at the next safe point.
    start: Cell<bool>,
    /// No unpark before this time (ms) after one found no memory, and the
    /// wait before the next.
    retry_at: Cell<u64>,
    wait: Cell<u64>,
    /// Its batch was refused, or it was stopped while parked: never made.
    refused: Cell<bool>,
    stopped: Cell<bool>,
}

/// The inside of a lease. The parked part is boxed: a live plugin's lease
/// pays one pointer for it, not its fields (Senior M2 on #1308).
pub(crate) struct Slot {
    live: OnceCell<Box<dyn NativeInstance>>,
    parked: RefCell<Option<Box<Parked>>>,
}

impl Slot {
    pub(crate) fn live_now(instance: Box<dyn NativeInstance>) -> Self {
        let live = OnceCell::new();
        let _ = live.set(instance);
        Self {
            live,
            parked: RefCell::new(None),
        }
    }

    /// `None` where the heap has no room for the parked part.
    pub(crate) fn parked(recipe: Box<dyn Unpark>, clock: CallbackRuntime) -> Option<Self> {
        let parked = super::shared::try_box(Parked {
            recipe,
            clock,
            start: Cell::new(false),
            retry_at: Cell::new(0),
            wait: Cell::new(0),
            refused: Cell::new(false),
            stopped: Cell::new(false),
        })?;
        Some(Self {
            live: OnceCell::new(),
            parked: RefCell::new(Some(parked)),
        })
    }

    /// Makes a parked plugin now where this is a safe point; elsewhere it
    /// stays parked and is made at its first call, as ever. `Ok` for a
    /// live plugin.
    ///
    /// # Errors
    /// As [`Self::live`] at a safe point (`ERR_BUSY` while its wait after
    /// no memory runs, `ERR_CANCELLED` for a refused or stopped plugin).
    pub(crate) fn make_if_safe(&self) -> Result<(), i32> {
        let safe = self.live.get().is_none()
            && self
                .parked
                .borrow()
                .as_ref()
                .is_some_and(|parked| parked.clock.safe_point());
        if safe {
            self.live().map(|_| ())
        } else {
            Ok(())
        }
    }

    /// The live instance without making it: `None` while parked.
    pub(crate) fn peek(&self) -> Option<&dyn NativeInstance> {
        self.live.get().map(|instance| &**instance)
    }

    /// Whether the plugin is parked (not made yet, not refused or stopped).
    pub(crate) fn is_parked(&self) -> bool {
        self.live.get().is_none()
            && self
                .parked
                .borrow()
                .as_ref()
                .is_some_and(|parked| !parked.refused.get() && !parked.stopped.get())
    }

    /// The accessor every call into the plugin goes through: the live
    /// instance, made now where it is parked and this is a safe point.
    ///
    /// # Errors
    /// `ERR_CANCELLED` for a stopped plugin or one whose batch was refused;
    /// `ERR_BUSY` for a parked plugin that cannot be made now (asked to
    /// start on the next pass, never treated as stopped).
    pub(crate) fn live(&self) -> Result<&dyn NativeInstance, i32> {
        if let Some(instance) = self.live.get() {
            return if instance.running() {
                Ok(&**instance)
            } else {
                Err(ERR_CANCELLED)
            };
        }
        self.unpark()?;
        self.live.get().map(|instance| &**instance).ok_or(ERR_BUSY)
    }

    /// Makes the parked plugin now where it may be made.
    fn unpark(&self) -> Result<(), i32> {
        let made = {
            let parked = self.parked.borrow();
            let Some(parked) = parked.as_ref() else {
                return Err(ERR_CANCELLED);
            };
            let now_ms = parked.clock.now_ms();
            if parked.refused.get() || parked.stopped.get() {
                return Err(ERR_CANCELLED);
            }
            if !parked.clock.safe_point() || now_ms < parked.retry_at.get() {
                parked.start.set(true);
                return Err(ERR_BUSY);
            }
            // Its own registration transaction holds off any other unpark.
            let _hold = parked.clock.hold();
            match parked.recipe.unpark() {
                Ok(instance) => instance,
                Err(Unparked::NoMemory) => {
                    let wait = parked
                        .wait
                        .get()
                        .saturating_mul(2)
                        .clamp(RETRY_FIRST_MS, RETRY_LAST_MS);
                    parked.wait.set(wait);
                    parked.retry_at.set(now_ms.saturating_add(wait));
                    parked.start.set(true);
                    return Err(ERR_BUSY);
                }
                Err(Unparked::Refused) => {
                    parked.refused.set(true);
                    parked.start.set(true);
                    return Err(ERR_CANCELLED);
                }
            }
        };
        if self.live.set(made).is_err() {
            // Never: only this path sets it, and only once.
            return Err(ERR_BUSY);
        }
        // Made: the recipe and its retry state give their heap back.
        self.parked.replace(None);
        Ok(())
    }

    /// Whether a start was asked and is due now, or a refused batch
    /// waits to be withdrawn; clears the request.
    pub(crate) fn take_start(&self) -> bool {
        self.parked.borrow().as_ref().is_some_and(|parked| {
            let due = parked.refused.get() || parked.clock.now_ms() >= parked.retry_at.get();
            due && parked.start.replace(false)
        })
    }

    /// When a start asked for may be made (ms; now for a refused batch to
    /// withdraw): a wake deadline, so what waits for it is not left to the
    /// next key press (#248). `None` where none was asked, and once the wait
    /// after no memory reached its longest: the device no longer wakes for
    /// it (six tries in about a minute), what waits is kept and tried again
    /// at the next pass anything else brings (Senior on #1337).
    pub(crate) fn start_at(&self) -> Option<u64> {
        let parked = self.parked.borrow();
        let parked = parked
            .as_ref()
            .filter(|parked| parked.start.get() && parked.wait.get() < RETRY_LAST_MS)?;
        Some(if parked.refused.get() {
            0
        } else {
            parked.retry_at.get()
        })
    }

    /// Whether the plugin gave up: its wait after no memory reached its
    /// longest (a real make failed each time). What waits for it is then
    /// dropped (Senior and Manager on #1337), and the wait starts again from
    /// the first, so a later use tries at once. `false` otherwise.
    pub(crate) fn give_up(&self) -> bool {
        let parked = self.parked.borrow();
        let Some(parked) = parked.as_ref().filter(|p| p.wait.get() >= RETRY_LAST_MS) else {
            return false;
        };
        parked.wait.set(0);
        parked.retry_at.set(0);
        parked.start.set(false);
        true
    }

    /// Whether the batch was refused at an unpark (the row withdraws it).
    pub(crate) fn refused(&self) -> bool {
        self.parked
            .borrow()
            .as_ref()
            .is_some_and(|parked| parked.refused.get())
    }

    pub(crate) fn running(&self) -> bool {
        match self.live.get() {
            Some(instance) => instance.running(),
            None => self.is_parked(),
        }
    }

    pub(crate) fn stop(&self) {
        if let Some(instance) = self.live.get() {
            instance.stop();
        } else if let Some(parked) = self.parked.borrow().as_ref() {
            parked.stopped.set(true);
        }
    }

    /// Revoked before the owner's cleanup: a parked plugin has no instance
    /// to retire; it is never made after this.
    pub(crate) fn retire(&self, owner: u64) -> Result<(), i32> {
        match self.live.get() {
            Some(instance) => instance.retire(owner),
            None => {
                self.stop();
                Ok(())
            }
        }
    }

    pub(crate) fn subscribed(&self, tag: u32) -> bool {
        match self.live.get() {
            Some(instance) => instance.subscribed(tag),
            None => self
                .parked
                .borrow()
                .as_ref()
                .is_some_and(|parked| parked.recipe.subscribed(tag)),
        }
    }

    pub(crate) fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        match self.live.get() {
            Some(instance) => instance.with_image(visit),
            None => match self.parked.borrow().as_ref() {
                Some(parked) => parked.recipe.with_image(visit),
                None => visit(None),
            },
        }
    }
}

#[cfg(test)]
#[path = "lease_slot_tests.rs"]
mod tests;
