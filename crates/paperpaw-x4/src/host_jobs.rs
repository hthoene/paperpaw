//! What the X4 family hands the kernel for plugin jobs and firmware updates
//! (`docs/abi/device.md` 5.1 to 5.4): a store's install, update and removal of
//! a plugin, a plugin check, and an update of the firmware itself. The jobs
//! share one queue in the kernel, whose firmware host is the device's
//! ([`SharedUpdates`] over the OTA the shell also drives, or [`NoFirmware`]
//! where the device has no second app partition), and the plugin host is what
//! the loader itself reads and offers a card plugin ([`plugin_host`]).
//!
//! One place for the firmware (`shells/xteink`) and the emulator's profiles of
//! these devices (`shells/desktop`): a device that wired these differently
//! would hide a gap from the host tests.
use alloc::rc::Rc;
use core::cell::RefCell;

use paperpaw_core::Kernel;
use paperpaw_native::header5::{Host as Loader, S3_PROFILE, X4_PROFILE};
use paperpaw_native::host_glue::MINOR;
use paperpaw_native::metadata;
use paperpaw_platform::Storage;
use paperpaw_platform::firmware::{CheckFacts, FirmwareHost, Reason, Running, Step};
use paperpaw_sdk::firmware::{CHIP_ESP32_C3, CHIP_ESP32_S3};

use crate::native_offer::{Wired, card_host};
use crate::ota::Flash;
use crate::updates::Updates;

/// Which chip family a device is of: it decides the plugin files the loader
/// reads and the chip an image is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// The ESP32-C3 devices (X4, X3).
    Riscv,
    /// The ESP32-S3 devices (X4 Pro, X4 Classic).
    Xtensa,
}

impl Family {
    /// The chip as an app image's header names it.
    #[must_use]
    pub const fn chip(self) -> u16 {
        match self {
            Self::Riscv => CHIP_ESP32_C3,
            Self::Xtensa => CHIP_ESP32_S3,
        }
    }

    /// What this family's loader reads of a plugin file (the same value the
    /// loader checks a file against, `shells/xteink/src/native/ppn5.rs`).
    #[must_use]
    pub const fn loader(self) -> Loader {
        match self {
            Self::Riscv => Loader::x4(MINOR),
            Self::Xtensa => Loader::x4_pro(MINOR),
        }
    }

    const fn profile(self) -> metadata::Profile<'static> {
        match self {
            Self::Riscv => X4_PROFILE,
            Self::Xtensa => S3_PROFILE,
        }
    }
}

/// What plugin jobs check a plugin with: what the loader reads and what the
/// kernel offers a card plugin once `wired` is wired, exactly the list the
/// loader admits a plugin against at the next start (`card_host`), so an
/// installed plugin is one that loads.
#[must_use]
pub fn plugin_host(family: Family, wired: Wired) -> (Loader, metadata::Host<'static>) {
    let loader = family.loader();
    let offered = card_host(wired, loader.abi_major, loader.minor, family.profile());
    (loader, offered)
}

/// Hands the kernel the plugin host of `family` once the services are wired,
/// before [`Kernel::start`], with the `loader` the shell's own loader reads
/// (`CodeLoader::reads`, the one source). `false`: nothing was set (the kernel was built
/// without a firmware host, whose queue plugin jobs share), so every plugin
/// job would be refused.
pub fn offer_plugin_jobs(
    kernel: &mut Kernel,
    loader: Loader,
    family: Family,
    wired: Wired,
) -> bool {
    let offered = card_host(wired, loader.abi_major, loader.minor, family.profile());
    kernel.set_plugin_host(loader, offered, &[])
}

/// The firmware host of a device that installs no firmware: every firmware
/// job is refused as on a device shared with another firmware
/// (`Reason::NOT_HERE`, room 0), which plugin jobs sharing its queue never call.
#[derive(Debug, Clone, Copy)]
pub struct NoFirmware {
    chip: u16,
}

impl NoFirmware {
    /// A host for the device of `family`.
    #[must_use]
    pub const fn new(family: Family) -> Self {
        Self {
            chip: family.chip(),
        }
    }
}

impl FirmwareHost for NoFirmware {
    fn running(&self) -> Running<'_> {
        Running::new(env!("CARGO_PKG_VERSION"), "paperpaw", self.chip, 0, false)
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

/// The device's [`Updates`], shared by the shell (the trial, the interface-6
/// flow, the start) and the kernel's firmware jobs: one OTA state, never two.
/// A step that finds it in use by the other side ends as `Reason::OTHER`: both
/// run in turn on the main loop, so this is a refusal that cannot happen
/// there, and never a panic.
pub struct SharedUpdates<F> {
    updates: Rc<RefCell<Updates<F>>>,
    version: &'static str,
    project: &'static str,
}

impl<F> core::fmt::Debug for SharedUpdates<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SharedUpdates").finish_non_exhaustive()
    }
}

impl<F: Flash> SharedUpdates<F> {
    /// The kernel's side of `updates`; `version` and `project` as `updates`
    /// was made with, kept here to hand out ([`FirmwareHost::running`]
    /// borrows from its host).
    #[must_use]
    pub fn new(
        updates: Rc<RefCell<Updates<F>>>,
        version: &'static str,
        project: &'static str,
    ) -> Self {
        Self {
            updates,
            version,
            project,
        }
    }
}

impl<F: Flash> FirmwareHost for SharedUpdates<F> {
    fn running(&self) -> Running<'_> {
        match self.updates.try_borrow() {
            Ok(updates) => {
                let now = FirmwareHost::running(&*updates);
                Running::new(self.version, self.project, now.chip, now.room, now.undone)
            }
            // Not told: no room for an image, so none is offered.
            Err(_) => Running::new(self.version, self.project, 0, 0, false),
        }
    }

    fn begin_check(&mut self, path: &str) -> Result<(), Reason> {
        self.updates
            .try_borrow_mut()
            .map_err(|_| Reason::OTHER)?
            .begin_check(path)
    }

    fn check_step(
        &mut self,
        storage: &dyn Storage,
        built_in: &mut dyn FnMut(&str),
    ) -> Step<CheckFacts> {
        match self.updates.try_borrow_mut() {
            Ok(mut updates) => updates.check_step(storage, built_in),
            Err(_) => Step::Failed(Reason::OTHER),
        }
    }

    fn begin_write(
        &mut self,
        path: &str,
        digest: [u8; 32],
        key: Option<[u8; 32]>,
        foreign: bool,
    ) -> Result<(), Reason> {
        self.updates
            .try_borrow_mut()
            .map_err(|_| Reason::OTHER)?
            .begin_write(path, digest, key, foreign)
    }

    fn write_step(&mut self, storage: &dyn Storage) -> Step<()> {
        match self.updates.try_borrow_mut() {
            Ok(mut updates) => updates.write_step(storage),
            Err(_) => Step::Failed(Reason::OTHER),
        }
    }

    fn readback_step(&mut self) -> Step<()> {
        match self.updates.try_borrow_mut() {
            Ok(mut updates) => updates.readback_step(),
            Err(_) => Step::Failed(Reason::OTHER),
        }
    }

    fn commit(&mut self) -> Result<(), Reason> {
        self.updates
            .try_borrow_mut()
            .map_err(|_| Reason::OTHER)?
            .commit()
    }

    fn abandon(&mut self) {
        if let Ok(mut updates) = self.updates.try_borrow_mut() {
            updates.abandon();
        }
    }

    fn restart(&mut self) -> Result<(), Reason> {
        self.updates
            .try_borrow_mut()
            .map_err(|_| Reason::OTHER)?
            .restart()
    }

    fn dual_boot_capable(&self) -> bool {
        self.updates
            .try_borrow()
            .is_ok_and(|updates| updates.dual_boot_capable())
    }

    fn dual_boot_declared(&self) -> bool {
        self.updates
            .try_borrow()
            .is_ok_and(|updates| updates.dual_boot_declared())
    }

    fn restart_running(&mut self) -> Result<(), Reason> {
        self.updates
            .try_borrow_mut()
            .map_err(|_| Reason::OTHER)?
            .restart_running()
    }
}
