//! The firmware commands of the DEV stage (`docs/abi/device.md` 3, 5.1, 5.3):
//! `install_firmware` (1578), `firmware_restart` (1579) and `check_file`
//! (1592) of kinds 0 (a plugin folder) and 1 (a firmware image).
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
//! * **Plugin checks** (`check_file` kind 0, feature `uses.install`) are a job
//!   of the same queue ([`Driver::submit_plugin`]); without a plugin host
//!   they are `ERR_UNSUPPORTED`. A kind beyond 1 is `ERR_DENIED`. A plugin
//!   install and removal are not commands here yet (`BLOCKED 5.2-install`).
use super::{CommandStage, DevOp, WireCommand};
use crate::native_host::{
    dual_boot_ini::Mode,
    firmware_cell::{DriverRef, FirmwareCell},
    owner_paths,
    plugin_check::PluginCheck,
    plugin_install::{self, PluginInstall},
    plugin_jobs::PluginJobs,
    plugin_remove::PluginRemove,
    plugin_rules::{Author, Evidence, Rotation},
    shared::try_box,
};
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::{cell::Cell, fmt};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_sdk::native::keys::Revocation;

/// At most one firmware command in a batch.
pub(super) const MAX_PER_BATCH: usize = 1;
/// At most this many `trust_revoked` in a batch (`device.md` 3).
pub(super) const MAX_REVOKED_PER_BATCH: usize = 8;
const FEATURE_FIRMWARE: &str = "uses.firmware";
/// The feature key of plugin installs, and of answering plugin requests.
pub const FEATURE_INSTALL: &str = "uses.install";
/// `install_firmware.flags` bits 0 (older) and 1 (foreign).
const FLAGS: u32 = 3;
/// `check_file.kind` of a plugin and of a firmware image.
const KIND_PLUGIN: u32 = 0;
const KIND_FIRMWARE: u32 = 1;

/// The device's firmware and the submitting plugin's owner key.
#[derive(Clone)]
pub(crate) struct FirmwareContext {
    firmware: Rc<FirmwareCell>,
    pub(crate) owner: u64,
    /// The driver's count of what was skipped, kept here to be reachable
    /// when the driver itself is borrowed.
    skipped: Rc<Cell<u32>>,
}

impl FirmwareContext {
    /// The context of `owner` over the device's `firmware`.
    pub(crate) fn new(firmware: Rc<FirmwareCell>, owner: u64) -> Self {
        let skipped = firmware.skip_counter();
        Self {
            firmware,
            owner,
            skipped,
        }
    }

    /// The firmware table, made now where none is: a command that submits.
    /// `ERR_NO_MEMORY` where there is no room for it.
    pub(crate) fn driver(&self) -> Result<DriverRef, i32> {
        self.firmware.make()
    }

    /// The firmware table, where one is made (none: no request is known).
    pub(crate) fn existing(&self) -> Option<DriverRef> {
        self.firmware.driver()
    }

    /// Whether plugin jobs are taken, made or not.
    fn plugins_accepted(&self) -> bool {
        self.firmware.plugins_accepted()
    }

    fn skip(&self) {
        self.skipped.set(self.skipped.get().saturating_add(1));
    }
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
    /// `check_file` of a plugin folder: a plugin job of the queue.
    PluginCheck { request: u32, path: String },
    /// `remove_plugin`: a plugin job of the queue (the host asks the user).
    PluginRemove { request: u32, id: String },
    /// `install_plugin`: a plugin job of the queue, with the evidence that
    /// came with it in the batch (`install_author`, `install_rotation`).
    PluginInstall {
        request: u32,
        path: String,
        flags: u32,
        evidence: Box<Evidence>,
    },
    /// `firmware_restart`: the host asks the user.
    Restart(FirmwareContext),
    /// `firmware_dual_boot`: a job of the queue (mode 1 asks the user).
    DualBoot { request: u32, mode: Mode },
    /// `trust_revoked`: handed to the driver, which keeps it if it holds.
    Revoke {
        ctx: FirmwareContext,
        revocation: Revocation,
    },
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
        let driver = self.ctx.existing();
        if let Some(Ok(mut driver)) = driver.as_ref().map(|driver| driver.try_borrow_mut()) {
            for request in &self.requests {
                driver.cancel(self.ctx.owner, *request);
            }
        } else {
            // The table is borrowed (never from a callback): the requests
            // stay and run; counted, not silent.
            self.ctx.skip();
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
            | WireCommand::FirmwareDualBoot { .. }
            | WireCommand::InstallPlugin { .. }
            | WireCommand::RemovePlugin { .. }
    )
}

/// Whether the command is evidence for the `install_plugin` before it.
pub(super) fn is_evidence(command: &WireCommand<'_>) -> bool {
    matches!(
        command,
        WireCommand::InstallAuthor { .. } | WireCommand::InstallRotation { .. }
    )
}

/// Most `install_rotation` of one request (`device.md` 3).
const ROTATIONS_PER_REQUEST: u8 = plugin_rules_max();

const fn plugin_rules_max() -> u8 {
    crate::native_host::plugin_rules::ROTATIONS_MAX as u8
}

/// What the batch has said of its install so far: the request it belongs to
/// and how much evidence came after it.
#[derive(Clone, Copy, Default)]
pub(super) struct EvidenceSeen {
    request: Option<u32>,
    author: bool,
    rotations: u8,
}

impl EvidenceSeen {
    /// An `install_plugin` of this request starts a new evidence record.
    pub(super) fn install(&mut self, request: u32) {
        *self = Self {
            request: Some(request),
            ..Self::default()
        };
    }

    /// Another device command stands between an install and what follows:
    /// evidence after it belongs to no install (`dev_push` would only refuse
    /// it later, with the batch half taken).
    pub(super) fn other_command(&mut self) {
        *self = Self::default();
    }

    /// Counts one evidence command: it must name the batch's install, at most
    /// one author and [`ROTATIONS_PER_REQUEST`] rotations.
    pub(super) fn take(&mut self, command: &WireCommand<'_>) -> Result<(), i32> {
        match *command {
            WireCommand::InstallAuthor { request, .. } => {
                if self.request != Some(request) || self.author {
                    return Err(ERR_INVALID);
                }
                self.author = true;
            }
            WireCommand::InstallRotation { request, .. } => {
                if self.request != Some(request) || self.rotations >= ROTATIONS_PER_REQUEST {
                    return Err(ERR_INVALID);
                }
                self.rotations += 1;
            }
            _ => {}
        }
        Ok(())
    }
}

/// The feature key the command needs.
pub(super) fn area(command: &WireCommand<'_>) -> Option<&'static str> {
    match command {
        WireCommand::InstallFirmware { .. }
        | WireCommand::FirmwareRestart {}
        | WireCommand::FirmwareDualBoot { .. } => Some(FEATURE_FIRMWARE),
        WireCommand::CheckFile { kind, .. } if *kind == KIND_PLUGIN => Some(FEATURE_INSTALL),
        WireCommand::InstallPlugin { .. }
        | WireCommand::RemovePlugin { .. }
        | WireCommand::InstallAuthor { .. }
        | WireCommand::InstallRotation { .. }
        | WireCommand::TrustRevoked { .. } => Some(FEATURE_INSTALL),
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

    /// The key and the signature of a `trust_revoked` are whole; whether it
    /// holds is the store's rule, checked where it is kept.
    fn validate_revocation(
        &self,
        plugin: &str,
        key: &[u8],
        signature: &[u8],
    ) -> Result<usize, i32> {
        let _ = self.firmware()?;
        if plugin.is_empty() || key.len() != 32 || signature.len() != 64 {
            return Err(ERR_INVALID);
        }
        Ok(plugin.len())
    }

    /// Keys and the digest of an `install_author` are none or whole, an
    /// `install_rotation`'s are (`device.md` 5.2).
    fn validate_evidence(&self, command: &WireCommand<'_>) -> Result<usize, i32> {
        let _ = self.firmware()?;
        let whole = match *command {
            WireCommand::InstallAuthor {
                author,
                recovery,
                sha256,
                ..
            } => [author, recovery, sha256]
                .iter()
                .all(|bytes| matches!(bytes.len(), 0 | 32)),
            WireCommand::InstallRotation { to, signature, .. } => {
                to.len() == 32 && signature.len() == 64
            }
            _ => false,
        };
        if whole { Ok(0) } else { Err(ERR_INVALID) }
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
                    KIND_PLUGIN if request == 0 => Err(ERR_INVALID),
                    KIND_PLUGIN => {
                        if !self.firmware()?.plugins_accepted() {
                            return Err(ERR_UNSUPPORTED);
                        }
                        self.firmware_path(root, path).map(|joined| joined.len())
                    }
                    KIND_FIRMWARE if request == 0 => Err(ERR_INVALID),
                    KIND_FIRMWARE => self.firmware_path(root, path).map(|joined| joined.len()),
                    _ => Err(ERR_DENIED),
                }
            }
            WireCommand::InstallPlugin {
                request,
                root,
                path,
                flags,
            } => {
                let ctx = self.firmware()?;
                if request == 0 || flags & !plugin_install::FLAGS != 0 {
                    return Err(ERR_INVALID);
                }
                if !ctx.plugins_accepted() {
                    return Err(ERR_UNSUPPORTED);
                }
                self.firmware_path(root, path).map(|joined| joined.len())
            }
            // The plugin's id is checked by the job (a name no plugin has, a
            // built-in: reason 15); `flags` is reserved.
            WireCommand::RemovePlugin {
                request,
                plugin,
                flags,
            } => {
                let ctx = self.firmware()?;
                if request == 0 || flags != 0 || plugin.is_empty() {
                    return Err(ERR_INVALID);
                }
                if !ctx.plugins_accepted() {
                    return Err(ERR_UNSUPPORTED);
                }
                Ok(plugin.len())
            }
            WireCommand::InstallAuthor { .. } | WireCommand::InstallRotation { .. } => {
                self.validate_evidence(command)
            }
            WireCommand::TrustRevoked {
                plugin,
                key,
                signature,
            } => self.validate_revocation(plugin, key, signature),
            WireCommand::FirmwareRestart {} => {
                let ctx = self.firmware()?;
                // None made: no image waits for its restart.
                let driver = ctx.existing().ok_or(ERR_STATE)?;
                let mut driver = driver.try_borrow_mut().map_err(|_| ERR_BUSY)?;
                driver.restart_gate(ctx.owner).map(|()| 0)
            }
            // Answered as unsupported until the host opens it
            // ([`Driver::open_dual_boot`]); a mode beyond 1 is `ERR_INVALID`.
            WireCommand::FirmwareDualBoot { request, mode } => {
                let ctx = self.firmware()?;
                // A device that boots two firmwares keeps its driver made.
                let driver = ctx.existing().ok_or(ERR_UNSUPPORTED)?;
                let driver = driver.try_borrow().map_err(|_| ERR_BUSY)?;
                if !driver.dual_boot_open() || !driver.plugins_accepted() {
                    return Err(ERR_UNSUPPORTED);
                }
                if request == 0 || Mode::of(mode).is_none() {
                    return Err(ERR_INVALID);
                }
                Ok(0)
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
                kind,
            } if kind == KIND_PLUGIN => Ok(DevOp::Firmware(FirmwareOp::PluginCheck {
                request,
                path: self.firmware_path(root, path)?,
            })),
            WireCommand::InstallPlugin {
                request,
                root,
                path,
                flags,
            } => Ok(DevOp::Firmware(FirmwareOp::PluginInstall {
                request,
                path: self.firmware_path(root, path)?,
                flags,
                evidence: try_box(Evidence::NONE).ok_or(ERR_NO_MEMORY)?,
            })),
            WireCommand::RemovePlugin {
                request, plugin, ..
            } => {
                let mut id = String::new();
                id.try_reserve_exact(plugin.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                id.push_str(plugin);
                Ok(DevOp::Firmware(FirmwareOp::PluginRemove { request, id }))
            }
            WireCommand::TrustRevoked {
                plugin,
                key,
                signature,
            } => {
                let (Ok(key), Ok(signature)) = (key.try_into(), signature.try_into()) else {
                    return Err(ERR_INVALID);
                };
                let mut id = String::new();
                id.try_reserve_exact(plugin.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                id.push_str(plugin);
                Ok(DevOp::Firmware(FirmwareOp::Revoke {
                    ctx: self.firmware()?.clone(),
                    revocation: Revocation::new(id, key, signature),
                }))
            }
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
            WireCommand::FirmwareDualBoot { request, mode } => {
                Ok(DevOp::Firmware(FirmwareOp::DualBoot {
                    request,
                    mode: Mode::of(mode).ok_or(ERR_INVALID)?,
                }))
            }
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// Adds an `install_author` or an `install_rotation` to the install before
    /// it in the batch (validated: the request matches, the counts hold).
    pub(super) fn firmware_evidence(&mut self, command: &WireCommand<'_>) -> Result<(), i32> {
        let dev = self.dev.as_mut().ok_or(ERR_UNSUPPORTED)?;
        let Some(DevOp::Firmware(FirmwareOp::PluginInstall { evidence, .. })) =
            dev.items.last_mut().map(|item| &mut item.op)
        else {
            return Err(ERR_INVALID);
        };
        match *command {
            WireCommand::InstallAuthor {
                author,
                recovery,
                sha256,
                ..
            } => {
                // None or whole (validated): a digest that does not match the
                // staged code is reason 18.
                evidence.author = Some(Author {
                    author: author.try_into().ok(),
                    recovery: recovery.try_into().ok(),
                    sha256: sha256.try_into().ok(),
                });
            }
            WireCommand::InstallRotation { to, signature, .. } => {
                let (Ok(to), Ok(signature)) = (to.try_into(), signature.try_into()) else {
                    return Err(ERR_INVALID);
                };
                if !evidence.push_rotation(Rotation { to, signature }) {
                    return Err(ERR_INVALID);
                }
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
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
                    DevOp::Firmware(
                        FirmwareOp::Install { .. }
                            | FirmwareOp::Check { .. }
                            | FirmwareOp::PluginCheck { .. }
                            | FirmwareOp::PluginInstall { .. }
                            | FirmwareOp::PluginRemove { .. }
                            | FirmwareOp::DualBoot { .. }
                    )
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
        let driver = ctx.driver()?;
        let mut driver = driver.try_borrow_mut().map_err(|_| ERR_BUSY)?;
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
                DevOp::Firmware(FirmwareOp::DualBoot { request, mode }) => driver
                    .submit_dual_boot(ctx.owner, *request, *mode)
                    .map(|()| *request),
                DevOp::Firmware(FirmwareOp::PluginCheck { request, path }) => {
                    PluginCheck::new(ctx.owner, *request, path)
                        .ok_or(ERR_NO_MEMORY)
                        .and_then(|job| driver.submit_plugin(PluginJobs::Check(job)))
                        .map(|()| *request)
                }
                DevOp::Firmware(FirmwareOp::PluginRemove { request, id }) => {
                    PluginRemove::new(ctx.owner, *request, id)
                        .ok_or(ERR_NO_MEMORY)
                        .and_then(|job| driver.submit_plugin(PluginJobs::Remove(job)))
                        .map(|()| *request)
                }
                DevOp::Firmware(FirmwareOp::PluginInstall {
                    request,
                    path,
                    flags,
                    evidence,
                }) => PluginInstall::new(ctx.owner, *request, path, *flags, **evidence)
                    .ok_or(ERR_NO_MEMORY)
                    .and_then(|job| driver.submit_plugin(PluginJobs::Install(job)))
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
            FirmwareOp::Install { request, .. }
            | FirmwareOp::Check { request, .. }
            | FirmwareOp::PluginCheck { request, .. }
            | FirmwareOp::PluginInstall { request, .. }
            | FirmwareOp::PluginRemove { request, .. }
            | FirmwareOp::DualBoot { request, .. } => {
                if let Some(release) = fw {
                    release.applied(request);
                }
            }
            FirmwareOp::Revoke { ctx, revocation } => {
                // Nothing answers it: one that finds no room is counted, and
                // the library lists it again.
                let queued = ctx.driver().is_ok_and(|driver| {
                    driver
                        .try_borrow_mut()
                        .is_ok_and(|mut driver| driver.queue_revocation(revocation))
                });
                if !queued {
                    ctx.skip();
                }
            }
            FirmwareOp::Restart(ctx) => {
                // The state may have moved since submit: then nothing asks,
                // and the skip is counted.
                let asked = ctx.existing().ok_or(ERR_STATE).and_then(|driver| {
                    driver
                        .try_borrow_mut()
                        .map_err(|_| ERR_BUSY)
                        .and_then(|mut driver| driver.request_restart(ctx.owner))
                });
                if asked.is_err() {
                    ctx.skip();
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "commands_dev_firmware_tests.rs"]
mod tests;
