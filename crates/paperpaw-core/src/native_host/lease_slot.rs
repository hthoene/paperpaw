//! The inside of a [`NativeLease`](super::native_instances::NativeLease): the
//! plugin's live instance, or, for a parked plugin, the recipe that makes it
//! again (#248, `docs/measurements/parked-plugins.md`).
//!
//! A parked plugin is running for every plugin and for itself: the one
//! accessor, [`Slot::live`], makes it again at a safe point (no callback
//! running, no event batch being delivered, no registration transaction open)
//! and answers `ERR_BUSY` anywhere else, the plugin asked to start on the next
//! pass. It never answers "stopped" for a parked plugin. A plugin registered
//! parked keeps its recipe while it runs, and goes back to parked
//! ([`Slot::repark`], D3) only where nothing holds it: the live instance is
//! lent as a [`Ref`], and parking again takes the slot with `try_borrow_mut`,
//! so while any call holds the instance the plugin stays running.
use super::{native_callbacks::CallbackRuntime, native_instances::NativeInstance};
use alloc::boxed::Box;
use core::cell::{Cell, Ref, RefCell};
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
    /// Every access goes through `try_borrow` / `try_borrow_mut` (a test
    /// greps for it): a borrow that fails answers `ERR_BUSY`, never panics.
    live: RefCell<Option<Box<dyn NativeInstance>>>,
    /// A plugin registered parked keeps this while it runs (its recipe, to
    /// park it again); `None` for a plugin registered awake.
    parked: RefCell<Option<Box<Parked>>>,
}

impl Slot {
    pub(crate) fn live_now(instance: Box<dyn NativeInstance>) -> Self {
        Self {
            live: RefCell::new(Some(instance)),
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
            live: RefCell::new(None),
            parked: RefCell::new(Some(parked)),
        })
    }

    /// The live instance, lent: `None` while parked, and while the slot is
    /// being parked again (never seen by a caller: that runs at safe points).
    fn instance(&self) -> Option<Ref<'_, dyn NativeInstance>> {
        let live = self.live.try_borrow().ok()?;
        Ref::filter_map(live, |live| live.as_deref()).ok()
    }

    /// Whether an instance is live (or the slot is busy being parked again).
    fn has_instance(&self) -> bool {
        self.live.try_borrow().map_or(true, |live| live.is_some())
    }

    /// Makes a parked plugin now where this is a safe point; elsewhere it
    /// stays parked and is made at its first call, as ever. `Ok` for a
    /// live plugin.
    ///
    /// # Errors
    /// As [`Self::live`] at a safe point (`ERR_BUSY` while its wait after
    /// no memory runs, `ERR_CANCELLED` for a refused or stopped plugin).
    pub(crate) fn make_if_safe(&self) -> Result<(), i32> {
        let safe = !self.has_instance()
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
    pub(crate) fn peek(&self) -> Option<Ref<'_, dyn NativeInstance>> {
        self.instance()
    }

    /// Whether the plugin is parked (not made yet, not refused or stopped).
    pub(crate) fn is_parked(&self) -> bool {
        !self.has_instance()
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
    pub(crate) fn live(&self) -> Result<Ref<'_, dyn NativeInstance>, i32> {
        if let Some(instance) = self.instance() {
            return if instance.running() {
                Ok(instance)
            } else {
                Err(ERR_CANCELLED)
            };
        }
        if self.live.try_borrow().is_err() {
            return Err(ERR_BUSY);
        }
        self.unpark()?;
        self.instance().ok_or(ERR_BUSY)
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
                    // Memory pressure: an idle plugin is parked again at the
                    // next safe point (D3, Decision 166).
                    parked.clock.note_pressure();
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
        // No `Ref` to an empty slot is ever kept, so this borrow succeeds;
        // where it does not, the instance made is dropped and the call waits.
        let Ok(mut live) = self.live.try_borrow_mut() else {
            return Err(ERR_BUSY);
        };
        if live.is_some() {
            return Err(ERR_BUSY);
        }
        *live = Some(made);
        // Made: the recipe stays, to park it again (D3); its start and retry
        // state start over.
        if let Some(parked) = self.parked.borrow().as_ref() {
            parked.start.set(false);
            parked.wait.set(0);
            parked.retry_at.set(0);
        }
        Ok(())
    }

    /// Whether the plugin was registered parked and runs now, outside every
    /// call of it: what [`Self::repark`] needs of the slot (the kernel checks
    /// the rest: idle owner, nothing of it on the stack or under way).
    pub(crate) fn may_repark(&self) -> bool {
        let registered_parked = self
            .parked
            .borrow()
            .as_ref()
            .is_some_and(|parked| !parked.refused.get() && !parked.stopped.get());
        registered_parked
            && self
                .instance()
                .is_some_and(|instance| instance.running() && instance.callback_depth() == 0)
    }

    /// Parks the plugin again (D3): its instance is dropped (its `destroy`
    /// runs, its owner is retired until it is made again, as at admission)
    /// and its recipe makes it at its next use. `false`, with nothing
    /// changed, where it may not ([`Self::may_repark`]) or any call holds
    /// the instance (the borrow fails). The instance is dropped after the
    /// slot's borrow ends, so its teardown never sees the slot borrowed.
    pub(crate) fn repark(&self) -> bool {
        if !self.may_repark() {
            return false;
        }
        let instance = {
            let Ok(mut slot) = self.live.try_borrow_mut() else {
                return false;
            };
            slot.take()
        };
        if let Some(parked) = self.parked.borrow().as_ref() {
            parked.start.set(false);
        }
        drop(instance);
        true
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
        match self.instance() {
            Some(instance) => instance.running(),
            None => self.is_parked(),
        }
    }

    pub(crate) fn stop(&self) {
        if let Some(instance) = self.instance() {
            instance.stop();
        }
        // A plugin stopped is never made (or parked) again.
        if let Some(parked) = self.parked.borrow().as_ref() {
            parked.stopped.set(true);
        }
    }

    /// Revoked before the owner's cleanup: a parked plugin has no instance
    /// to retire; it is never made after this.
    pub(crate) fn retire(&self, owner: u64) -> Result<(), i32> {
        match self.instance() {
            Some(instance) => instance.retire(owner),
            None => {
                self.stop();
                Ok(())
            }
        }
    }

    pub(crate) fn subscribed(&self, tag: u32) -> bool {
        match self.instance() {
            Some(instance) => instance.subscribed(tag),
            None => self
                .parked
                .borrow()
                .as_ref()
                .is_some_and(|parked| parked.recipe.subscribed(tag)),
        }
    }

    pub(crate) fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        match self.instance() {
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
