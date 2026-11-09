//! The firmware commands of the DEV stage (`docs/abi/device.md` 3, 5.1, 5.3):
//! `install_firmware` (1578), `firmware_restart` (1579) and `check_file`
//! (1592) of kind 1.
//!
//! * **Checked whole, then accepted at submit.** A feature not admitted is
//!   `ERR_DENIED`; without a firmware host every one is `ERR_UNSUPPORTED`;
//!   then at most one firmware command a batch, the flags (`ERR_INVALID`
//!   beyond bits 0 and 1), the key (none or 32 bytes), the root (0 or 1, 1
//!   with `uses.storage`) and the path (a card path the plugin may read), and
//!   the restart's gates (`ERR_STATE` with no image waiting, `ERR_DENIED` for
//!   another plugin). Then the request is accepted in the device's table
//!   ([`Driver::submit_install`], [`Driver::submit_check`]: `ERR_FULL`,
//!   `ERR_INVALID` for a number that waits, `ERR_NO_MEMORY`), all or nothing
//!   for the batch.
//! * **Applied after the callback.** An accepted request waits in the
//!   queue; nothing runs inside a callback. If the batch never reaches the
//!   drain (a callback error), [`FwRelease`] takes its requests back.
//! * **Plugin installs** (`check_file` kind 0) are `ERR_UNSUPPORTED` until the
//!   plugin check exists (`BLOCKED 5.2-install`); a kind beyond 1 is
//!   `ERR_DENIED`.
use super::{CommandStage, DevOp, WireCommand};
use crate::native_host::{firmware_driver::Driver, owner_paths};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::{cell::RefCell, fmt};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED,
};

/// At most one firmware command in a batch.
pub(super) const MAX_PER_BATCH: usize = 1;
const FEATURE_FIRMWARE: &str = "uses.firmware";
const FEATURE_INSTALL: &str = "uses.install";
/// `install_firmware.flags` bits 0 (older) and 1 (foreign).
const FLAGS: u32 = 3;
/// `check_file.kind` of a plugin and of a firmware image.
const KIND_PLUGIN: u32 = 0;
const KIND_FIRMWARE: u32 = 1;

/// The device's firmware table and the submitting plugin's owner key.
#[derive(Clone)]
pub(crate) struct FirmwareContext {
    pub(crate) driver: Rc<RefCell<Driver>>,
    pub(crate) owner: u64,
}

impl fmt::Debug for FirmwareContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FirmwareContext").finish_non_exhaustive()
    }
}

/// A firmware command to apply after the callback.
#[derive(Debug)]
pub(crate) enum FirmwareOp {
    /// `install_firmware`: accepted in the table by [`CommandStage::firmware_commit`].
    Install {
        request: u32,
        path: String,
        key: Option<[u8; 32]>,
        flags: u32,
    },
    /// `check_file` of a firmware image.
    Check { request: u32, path: String },
    /// `firmware_restart`: the host asks the user.
    Restart(FirmwareContext),
}

/// The requests a stage accepted and no drain applied yet; dropped, it takes
/// them back (a request that already runs is left to end).
pub(crate) struct FwRelease {
    ctx: FirmwareContext,
    requests: Vec<u32>,
}

impl fmt::Debug for FwRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FwRelease")
            .field("requests", &self.requests.len())
            .finish_non_exhaustive()
    }
}

impl FwRelease {
    fn applied(&mut self, request: u32) {
        self.requests.retain(|kept| *kept != request);
    }
}

impl Drop for FwRelease {
    fn drop(&mut self) {
        if self.requests.is_empty() {
            return;
        }
        if let Ok(mut driver) = self.ctx.driver.try_borrow_mut() {
            for request in &self.requests {
                driver.cancel(self.ctx.owner, *request);
            }
        }
    }
}

/// Whether the command is one of the firmware commands.
pub(super) fn is_firmware(command: &WireCommand<'_>) -> bool {
    matches!(
        command,
        WireCommand::InstallFirmware { .. }
            | WireCommand::FirmwareRestart {}
            | WireCommand::CheckFile { .. }
    )
}

/// The feature key the command needs.
pub(super) fn area(command: &WireCommand<'_>) -> Option<&'static str> {
    match command {
        WireCommand::InstallFirmware { .. } | WireCommand::FirmwareRestart {} => {
            Some(FEATURE_FIRMWARE)
        }
        WireCommand::CheckFile { kind, .. } if *kind == KIND_PLUGIN => Some(FEATURE_INSTALL),
        // A kind beyond 1 is refused by its own rule (`ERR_DENIED`).
        WireCommand::CheckFile { .. } => Some(FEATURE_FIRMWARE),
        _ => None,
    }
}

impl CommandStage<'_> {
    fn firmware(&self) -> Result<&FirmwareContext, i32> {
        self.dev
            .as_ref()
            .and_then(|dev| dev.ctx.firmware.as_ref())
            .ok_or(ERR_UNSUPPORTED)
    }

    /// The card path of `path` in `root`, for a plugin that may read it.
    fn firmware_path(&self, root: u32, path: &str) -> Result<alloc::string::String, i32> {
        if root > owner_paths::CARD || path.is_empty() {
            return Err(ERR_INVALID);
        }
        let features = self.dev.as_ref().ok_or(ERR_UNSUPPORTED)?.ctx.features;
        owner_paths::access_with(features, root, path, false)?;
        owner_paths::OwnerPaths::join(Some(self.owner.as_str()), root, path)
    }

    /// Checks one firmware command; the bytes it will copy.
    pub(super) fn validate_firmware(&self, command: &WireCommand<'_>) -> Result<usize, i32> {
        match *command {
            WireCommand::InstallFirmware {
                request,
                root,
                path,
                key,
                flags,
            } => {
                let _ = self.firmware()?;
                if request == 0 || flags & !FLAGS != 0 || !matches!(key.len(), 0 | 32) {
                    return Err(ERR_INVALID);
                }
                self.firmware_path(root, path).map(|joined| joined.len())
            }
            WireCommand::CheckFile {
                request,
                root,
                path,
                kind,
            } => {
                let _ = self.firmware()?;
                match kind {
                    KIND_PLUGIN => Err(ERR_UNSUPPORTED),
                    KIND_FIRMWARE if request == 0 => Err(ERR_INVALID),
                    KIND_FIRMWARE => self.firmware_path(root, path).map(|joined| joined.len()),
                    _ => Err(ERR_DENIED),
                }
            }
            WireCommand::FirmwareRestart {} => {
                let ctx = self.firmware()?;
                let driver = ctx.driver.try_borrow().map_err(|_| ERR_BUSY)?;
                driver.restart_gate(ctx.owner).map(|()| 0)
            }
            _ => Ok(0),
        }
    }

    /// The DEV item of a validated firmware command; an install or a check
    /// is accepted in the table later, by [`Self::firmware_commit`].
    pub(super) fn firmware_op(&self, command: &WireCommand<'_>) -> Result<DevOp, i32> {
        match *command {
            WireCommand::InstallFirmware {
                request,
                root,
                path,
                key,
                flags,
            } => Ok(DevOp::Firmware(FirmwareOp::Install {
                request,
                path: self.firmware_path(root, path)?,
                key: key.try_into().ok(),
                flags,
            })),
            WireCommand::CheckFile {
                request,
                root,
                path,
                ..
            } => Ok(DevOp::Firmware(FirmwareOp::Check {
                request,
                path: self.firmware_path(root, path)?,
            })),
            WireCommand::FirmwareRestart {} => Ok(DevOp::Firmware(FirmwareOp::Restart(
                self.firmware()?.clone(),
            ))),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// Accepts the installs and checks of the items from `start` in the
    /// device's table, all or none; what was accepted is given back when the
    /// stage drops without a drain.
    pub(super) fn firmware_commit(&mut self, start: usize) -> Result<(), i32> {
        let Some(dev) = self.dev.as_mut() else {
            return Ok(());
        };
        let new = dev.items.get(start..).unwrap_or_default();
        let wanted = new
            .iter()
            .filter(|item| {
                matches!(
                    item.op,
                    DevOp::Firmware(FirmwareOp::Install { .. } | FirmwareOp::Check { .. })
                )
            })
            .count();
        let Some(ctx) = dev.ctx.firmware.clone().filter(|_| wanted > 0) else {
            return Ok(());
        };
        let fw = dev.fw.get_or_insert_with(|| FwRelease {
            ctx: ctx.clone(),
            requests: Vec::new(),
        });
        fw.requests.try_reserve(wanted).map_err(|_| ERR_NO_MEMORY)?;
        let before = fw.requests.len();
        let mut driver = ctx.driver.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        for item in new {
            let accepted = match &item.op {
                DevOp::Firmware(FirmwareOp::Install {
                    request,
                    path,
                    key,
                    flags,
                }) => driver
                    .submit_install(ctx.owner, *request, path, *key, *flags)
                    .map(|()| *request),
                DevOp::Firmware(FirmwareOp::Check { request, path }) => driver
                    .submit_check(ctx.owner, *request, path)
                    .map(|()| *request),
                _ => continue,
            };
            match accepted {
                // Room was reserved above: this push cannot reallocate.
                Ok(request) => fw.requests.push(request),
                Err(error) => {
                    while fw.requests.len() > before {
                        if let Some(request) = fw.requests.pop() {
                            driver.cancel(ctx.owner, request);
                        }
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

impl crate::Kernel {
    /// Applies a firmware command after its callback: an accepted request is
    /// kept (the queue runs it), a restart asks the user.
    pub(super) fn apply_firmware(op: FirmwareOp, fw: &mut Option<FwRelease>) {
        match op {
            FirmwareOp::Install { request, .. } | FirmwareOp::Check { request, .. } => {
                if let Some(release) = fw {
                    release.applied(request);
                }
            }
            FirmwareOp::Restart(ctx) => {
                if let Ok(mut driver) = ctx.driver.try_borrow_mut() {
                    // The state may have moved since submit: then nothing asks.
                    let _ = driver.request_restart(ctx.owner);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "commands_dev_firmware_tests.rs"]
mod tests;
