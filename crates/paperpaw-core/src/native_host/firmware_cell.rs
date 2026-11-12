//! The device's firmware host and what plugin jobs need of it, with the
//! request table and install machine ([`Driver`], [`FirmwareInstall`]) made
//! only while a firmware or plugin job needs them (#2654, #248): a device that
//! offers installs pays for them from the first such command until nothing is
//! left to run, tell or keep, and not at idle.
//!
//! The kernel and the host views share one cell. Reading what a job left (a
//! request's state, a check's facts) never makes the driver: with none there is
//! nothing to read. A device that can boot two firmwares keeps its driver from
//! the start: its latch and its `paperpaw.ini` reading are the machine's.
use super::firmware_driver::{Driver, Kept, Lookup};
use super::firmware_install::FirmwareInstall;
use super::plugin_job::PluginHost;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::alloc::Layout;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::firmware::FirmwareHost;

/// What a driver is made of and keeps while none is made
/// ([`FirmwareCell`]): the device's host, what plugin jobs are told, the
/// grants' hook, and what plugins may still read of ended jobs (the outcomes
/// op 13 answers, and the machine while it keeps a check's result or a failed
/// install).
pub(crate) struct DriverParts {
    pub(super) host: Box<dyn FirmwareHost>,
    pub(super) plugin_host: Option<PluginHost>,
    pub(super) grants: Option<Box<dyn super::plugin_job::InstallGrants>>,
    pub(super) kept: Vec<Kept>,
    pub(super) machine: Option<Rc<RefCell<FirmwareInstall>>>,
}

impl DriverParts {
    pub(crate) fn new(host: Box<dyn FirmwareHost>) -> Self {
        Self {
            host,
            plugin_host: None,
            grants: None,
            kept: Vec::new(),
            machine: None,
        }
    }

    /// The plugin `owner` stopped: what was kept for it goes, and the
    /// machine with it once it keeps nothing.
    fn forget(&mut self, owner: u64) {
        self.kept.retain(|kept| kept.owner != owner);
        let rests = self.machine.as_ref().is_some_and(|machine| {
            machine.try_borrow_mut().is_ok_and(|mut machine| {
                machine.forget(owner);
                machine.at_rest()
            })
        });
        if rests {
            self.machine = None;
        }
    }

    pub(crate) fn dual_boot_capable(&self) -> bool {
        self.host.dual_boot_capable()
    }

    pub(crate) fn plugin_host(&self) -> Option<&PluginHost> {
        self.plugin_host.as_ref()
    }

    pub(crate) fn set_plugin_host(&mut self, host: PluginHost) {
        self.plugin_host = Some(host);
    }
}

/// The driver while it is made, or what it is made of while it is not.
enum State {
    Resting(DriverParts),
    Running(Rc<RefCell<Driver>>),
    /// Only while one is turned into the other.
    Moving,
}

pub(crate) struct FirmwareCell {
    state: RefCell<State>,
    /// The driver's count of skips, kept across its lives (see
    /// [`Driver::skip_counter`]).
    skipped: Rc<Cell<u32>>,
    /// A removal a cut left waits for the host to forget its plugin's
    /// secrets and grants: the kernel's next pass finishes it, made driver
    /// or not (`Kernel::native_firmware_pump`).
    resume: Cell<bool>,
}

impl FirmwareCell {
    /// The cell of the device's firmware `host`: resting, unless the device
    /// can boot two firmwares.
    pub(crate) fn new(host: Box<dyn FirmwareHost>) -> Self {
        let skipped = Rc::new(Cell::new(0));
        let parts = DriverParts::new(host);
        let state = if parts.dual_boot_capable() {
            State::Running(Rc::new(RefCell::new(Driver::from_parts(
                parts,
                Rc::new(RefCell::new(FirmwareInstall::new())),
                Rc::clone(&skipped),
            ))))
        } else {
            State::Resting(parts)
        };
        Self {
            state: RefCell::new(state),
            skipped,
            resume: Cell::new(false),
        }
    }

    /// A cell over a driver made already (tests of the commands).
    #[cfg(test)]
    pub(crate) fn running(driver: Rc<RefCell<Driver>>) -> Self {
        let skipped = driver
            .try_borrow()
            .map_or_else(|_| Rc::new(Cell::new(0)), |driver| driver.skip_counter());
        Self {
            state: RefCell::new(State::Running(driver)),
            skipped,
            resume: Cell::new(false),
        }
    }

    /// The driver, where one is made (nothing is made here).
    pub(crate) fn driver(&self) -> Option<Rc<RefCell<Driver>>> {
        match &*self.state.try_borrow().ok()? {
            State::Running(driver) => Some(Rc::clone(driver)),
            State::Resting(_) | State::Moving => None,
        }
    }

    /// The install machine: the driver's, or the one resting with what it
    /// keeps; `None` where none is (nothing to read) or in use.
    pub(crate) fn machine(&self) -> Option<Rc<RefCell<FirmwareInstall>>> {
        match &*self.state.try_borrow().ok()? {
            State::Running(driver) => Some(driver.try_borrow().ok()?.machine()),
            State::Resting(parts) => parts.machine.clone(),
            State::Moving => None,
        }
    }

    /// What op 13 says of `owner`'s request `request`, made or resting.
    ///
    /// # Errors
    /// `ERR_BUSY` where the cell or the driver is in use.
    pub(crate) fn lookup(&self, owner: u64, request: u32) -> Result<Option<Lookup>, i32> {
        match &*self.state.try_borrow().map_err(|_| ERR_BUSY)? {
            State::Running(driver) => Ok(driver
                .try_borrow()
                .map_err(|_| ERR_BUSY)?
                .lookup(owner, request)),
            State::Resting(parts) => Ok(kept_lookup(&parts.kept, owner, request)),
            State::Moving => Err(ERR_BUSY),
        }
    }

    /// The plugin `owner` stopped while no driver is made: what was kept for
    /// it goes (a made driver forgets it in `Driver::sender_stopped`).
    pub(crate) fn forget_resting(&self, owner: u64) {
        if let Ok(mut state) = self.state.try_borrow_mut()
            && let State::Resting(parts) = &mut *state
        {
            parts.forget(owner);
        }
    }

    /// The driver, made now where there is none: for a command that submits,
    /// checks or answers. `ERR_NO_MEMORY` where there is no room for it,
    /// `ERR_BUSY` where the cell is in use.
    pub(crate) fn make(&self) -> Result<Rc<RefCell<Driver>>, i32> {
        let mut state = self.state.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if let State::Running(driver) = &*state {
            return Ok(Rc::clone(driver));
        }
        // Rc has no fallible constructor: both blocks are asked for first,
        // held together and given back ([`room_for`]), in the order they are
        // made below: the machine (where none rests with the parts), then
        // the driver. Nothing allocates in between.
        let resting_machine = matches!(&*state, State::Resting(parts) if parts.machine.is_some());
        let driver = Layout::new::<RcBox<RefCell<Driver>>>();
        let machine = Layout::new::<RcBox<RefCell<FirmwareInstall>>>();
        if resting_machine {
            room_for(&[driver])?;
        } else {
            room_for(&[machine, driver])?;
        }
        let State::Resting(mut parts) = core::mem::replace(&mut *state, State::Moving) else {
            return Err(ERR_BUSY);
        };
        let machine = parts
            .machine
            .take()
            .unwrap_or_else(|| Rc::new(RefCell::new(FirmwareInstall::new())));
        let driver = Rc::new(RefCell::new(Driver::from_parts(
            parts,
            machine,
            Rc::clone(&self.skipped),
        )));
        *state = State::Running(Rc::clone(&driver));
        Ok(driver)
    }

    /// Lets the driver go where it is at rest ([`Driver::at_rest`]) and no one
    /// else holds it: what it was made of rests in the cell again. Whether
    /// none is made now.
    pub(crate) fn rest(&self) -> bool {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return false;
        };
        let State::Running(driver) = &*state else {
            return true;
        };
        if Rc::strong_count(driver) != 1
            || !driver.try_borrow().is_ok_and(|driver| driver.at_rest())
        {
            return false;
        }
        let State::Running(driver) = core::mem::replace(&mut *state, State::Moving) else {
            return false;
        };
        match Rc::try_unwrap(driver) {
            Ok(driver) => {
                *state = State::Resting(driver.into_inner().into_parts());
                true
            }
            Err(driver) => {
                *state = State::Running(driver);
                false
            }
        }
    }

    /// Whether plugin jobs are taken (a plugin host is set).
    pub(crate) fn plugins_accepted(&self) -> bool {
        match &*self.state.borrow() {
            State::Resting(parts) => parts.plugin_host().is_some(),
            State::Running(driver) => driver
                .try_borrow()
                .is_ok_and(|driver| driver.plugins_accepted()),
            State::Moving => false,
        }
    }

    /// What the device tells plugin jobs; `false` where the cell or its
    /// driver is in use.
    pub(crate) fn set_plugin_host(&self, host: PluginHost) -> bool {
        let Ok(state) = self.state.try_borrow_mut() else {
            return false;
        };
        let mut state = state;
        match &mut *state {
            State::Resting(parts) => {
                parts.set_plugin_host(host);
                true
            }
            State::Running(driver) => driver.try_borrow_mut().is_ok_and(|mut driver| {
                driver.set_plugin_host(host);
                true
            }),
            State::Moving => false,
        }
    }

    /// Asks the next pass to finish a removal a cut left (at start).
    pub(crate) fn want_resume(&self) {
        self.resume.set(true);
    }

    /// Whether a removal waits to be finished.
    pub(crate) fn resume_wanted(&self) -> bool {
        self.resume.get()
    }

    /// Takes the wish to finish a removal (once).
    pub(crate) fn take_resume(&self) -> bool {
        self.resume.take()
    }

    /// The count of skips, for a context that cannot borrow the driver.
    pub(crate) fn skip_counter(&self) -> Rc<Cell<u32>> {
        Rc::clone(&self.skipped)
    }
}

/// What op 13 says of `owner`'s ended request `request` in `kept`.
pub(crate) fn kept_lookup(kept: &[Kept], owner: u64, request: u32) -> Option<Lookup> {
    kept.iter()
        .filter(|kept| kept.owner == owner)
        .find_map(|kept| kept.outcome.filter(|outcome| outcome.request == request))
        .map(|outcome| Lookup {
            kind: outcome.kind,
            state: 2,
            outcome: Some(outcome),
            done: 0,
            total: 0,
        })
}

/// The block an `Rc` of `T` takes: its two counts and the value, as the
/// standard library lays it out (`repr(C)`, the counts first).
#[allow(dead_code, reason = "only its layout is used")]
#[repr(C)]
struct RcBox<T> {
    strong: Cell<usize>,
    weak: Cell<usize>,
    value: T,
}

/// Whether the blocks of `layouts` can be had now, in this order, each with
/// its own size and alignment: all are asked for and held at once, then
/// given back in the reverse order. `Rc::new` has no fallible form, so the
/// same requests in the same order follow at once, with nothing allocated
/// in between (one thread): the allocator is where it was before the probe
/// and finds the same room. Any refusal gives back what was had.
fn room_for(layouts: &[Layout]) -> Result<(), i32> {
    let mut held: [(*mut u8, Layout); 2] = [(core::ptr::null_mut(), Layout::new::<()>()); 2];
    let mut ok = layouts.len() <= held.len();
    for (slot, layout) in held.iter_mut().zip(layouts) {
        if !ok || layout.size() == 0 {
            continue;
        }
        // SAFETY: a nonzero size, checked above.
        let block = unsafe { alloc::alloc::alloc(*layout) };
        if block.is_null() {
            ok = false;
        } else {
            *slot = (block, *layout);
        }
    }
    for (block, layout) in held.iter().rev() {
        if !block.is_null() {
            // SAFETY: allocated above with this very layout, not freed yet.
            unsafe { alloc::alloc::dealloc(*block, *layout) };
        }
    }
    if ok { Ok(()) } else { Err(ERR_NO_MEMORY) }
}

#[cfg(test)]
#[path = "firmware_cell_tests.rs"]
mod tests;
