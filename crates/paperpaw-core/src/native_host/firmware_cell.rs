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
use super::shared::Shared;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::Storage;
use paperpaw_platform::firmware::{CheckFacts, FirmwareHost, Reason, Running, Step};

/// The driver, held by the cell and lent to the commands and the pump; made
/// with a fallible allocation, in place ([`FirmwareCell::make`]).
pub(crate) type DriverRef = Shared<RefCell<Driver>>;
/// The install machine, the driver's or resting with the parts.
pub(crate) type MachineRef = Shared<RefCell<FirmwareInstall>>;

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
    pub(super) machine: Option<MachineRef>,
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
    Running(DriverRef),
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
        let cell = Self {
            state: RefCell::new(State::Resting(DriverParts::new(host))),
            skipped: Rc::new(Cell::new(0)),
            resume: Cell::new(false),
        };
        // Where there is no room now, the pump makes it later
        // ([`Self::wants_driver`]).
        if cell.wants_driver() {
            let _ = cell.make();
        }
        cell
    }

    /// Whether a driver should be made though no command asked: a device
    /// that can boot two firmwares keeps one (its latch and `paperpaw.ini`
    /// reading are the machine's).
    pub(crate) fn wants_driver(&self) -> bool {
        self.state.try_borrow().is_ok_and(
            |state| matches!(&*state, State::Resting(parts) if parts.dual_boot_capable()),
        )
    }

    /// A cell over a driver made already (tests of the commands).
    #[cfg(test)]
    pub(crate) fn running(driver: DriverRef) -> Self {
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
    pub(crate) fn driver(&self) -> Option<DriverRef> {
        match &*self.state.try_borrow().ok()? {
            State::Running(driver) => driver.try_clone(),
            State::Resting(_) | State::Moving => None,
        }
    }

    /// The install machine: the driver's, or the one resting with what it
    /// keeps; `None` where none is (nothing to read) or in use.
    pub(crate) fn machine(&self) -> Option<MachineRef> {
        match &*self.state.try_borrow().ok()? {
            State::Running(driver) => driver.try_borrow().ok()?.machine(),
            State::Resting(parts) => parts.machine.as_ref().and_then(Shared::try_clone),
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
    pub(crate) fn make(&self) -> Result<DriverRef, i32> {
        let mut state = self.state.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if let State::Running(driver) = &*state {
            return driver.try_clone().ok_or(ERR_NO_MEMORY);
        }
        let State::Resting(mut parts) = core::mem::replace(&mut *state, State::Moving) else {
            return Err(ERR_BUSY);
        };
        // Each block is had with a fallible allocation and its value written
        // in place: the machine (where none rests with the parts), then the
        // driver. A refusal leaves the parts as they were.
        let (machine, rested) = match parts.machine.take() {
            Some(machine) => (machine, true),
            None => match MachineRef::try_new_from((), |()| RefCell::new(FirmwareInstall::new())) {
                Ok(machine) => (machine, false),
                Err(()) => {
                    *state = State::Resting(parts);
                    return Err(ERR_NO_MEMORY);
                }
            },
        };
        let made = DriverRef::try_new_from(
            (parts, machine, Rc::clone(&self.skipped)),
            |(parts, machine, skipped)| RefCell::new(Driver::from_parts(parts, machine, skipped)),
        );
        match made {
            Ok(driver) => {
                let lent = driver.try_clone().ok_or(ERR_NO_MEMORY);
                *state = State::Running(driver);
                lent
            }
            Err((mut parts, machine, _)) => {
                if rested {
                    parts.machine = Some(machine);
                }
                *state = State::Resting(parts);
                Err(ERR_NO_MEMORY)
            }
        }
    }

    /// Lets the driver go where it is at rest ([`Driver::at_rest`]) and no one
    /// else holds it: what it was made of rests in the cell again (taken out
    /// in place, the driver is not moved). Whether none is made now.
    pub(crate) fn rest(&self) -> bool {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return false;
        };
        let parts = {
            let State::Running(driver) = &*state else {
                return true;
            };
            if !driver.is_unique() || !driver.try_borrow().is_ok_and(|driver| driver.at_rest()) {
                return false;
            }
            let Ok(mut driver) = driver.try_borrow_mut() else {
                return false;
            };
            driver.take_parts()
        };
        *state = State::Resting(parts);
        true
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

/// A shared value for the tests' drivers and machines.
#[cfg(test)]
pub(crate) fn test_shared<T>(value: T) -> Shared<T> {
    match Shared::try_new(value) {
        Ok(shared) => shared,
        Err(_) => panic!("no room for a test value"),
    }
}

/// The host a driver keeps once its parts went back to the cell, until it
/// is dropped: never asked (a driver at rest runs nothing), every job
/// `NOT_HERE`. Zero-sized: its box allocates nothing.
pub(super) struct Gone;

impl FirmwareHost for Gone {
    fn running(&self) -> Running<'_> {
        Running::new("", "", 0, 0, false)
    }

    fn begin_check(&mut self, _: &str) -> Result<(), Reason> {
        Err(Reason::NOT_HERE)
    }

    fn check_step(&mut self, _: &dyn Storage, _: &mut dyn FnMut(&str)) -> Step<CheckFacts> {
        Step::Failed(Reason::NOT_HERE)
    }

    fn begin_write(
        &mut self,
        _: &str,
        _: [u8; 32],
        _: Option<[u8; 32]>,
        _: bool,
    ) -> Result<(), Reason> {
        Err(Reason::NOT_HERE)
    }

    fn write_step(&mut self, _: &dyn Storage) -> Step<()> {
        Step::Failed(Reason::NOT_HERE)
    }

    fn readback_step(&mut self) -> Step<()> {
        Step::Failed(Reason::NOT_HERE)
    }

    fn commit(&mut self) -> Result<(), Reason> {
        Err(Reason::NOT_HERE)
    }

    fn abandon(&mut self) {}

    fn restart(&mut self) -> Result<(), Reason> {
        Err(Reason::NOT_HERE)
    }
}

#[cfg(test)]
#[path = "firmware_cell_tests.rs"]
mod tests;
