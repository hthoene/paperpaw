//! Slot-15 DEV commands for power and Wi-Fi (`docs/abi/device.md` section 3):
//! commands 1564-1575 are checked and staged here, and applied by the kernel's
//! interface-6 commands after the callback.
//!
//! Staging (`CommandStage::stage`) refuses a whole batch before anything is
//! kept: a feature not admitted is `ERR_DENIED`, one the host does not
//! advertise `ERR_UNSUPPORTED`, then the batch limits, the per-command rules
//! and, last, the request table's reservation (`ERR_FULL`). Accepted commands
//! wait in [`DevBatch`], each tagged with the number of ordinary staged
//! commands before it, so [`Kernel::drain_native_dev`] applies everything in
//! batch order. Passwords live only in [`Secret`] (zeroed on drop, `Debug`
//! shows nothing) until the radio command is built at apply time.

use super::{CommandBatch, CommandStage, WireCommand, ui};

#[path = "commands_dev_firmware.rs"]
mod firmware;
use crate::native_host::{admitted_features::AdmittedFeatures, radio_requests::*};
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::{cell::RefCell, fmt, time::Duration};
pub use firmware::FEATURE_INSTALL;
pub(crate) use firmware::{FirmwareContext, FirmwareOp, FwRelease};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};
use paperpaw_sdk::{
    Command, Id, SettingValue, StackEntry, power::MODE_KIND, power::Wear, wifi::WifiCommand,
};

/// The feature key of commands 1564-1571.
pub const FEATURE_WIFI: &str = "uses.wifi";
/// The feature key of commands 1572-1575.
pub const FEATURE_POWER: &str = "uses.power";
/// The setting naming the plugin that is the network manager.
pub const NETWORK_MANAGER: &str = "core.network-manager";
/// A user input this recent (inclusive) backs a network change.
pub const INPUT_WINDOW_MS: u64 = 2_000;
/// Most Wi-Fi commands (1564-1571) in one batch.
pub const MAX_WIFI_PER_BATCH: usize = 2;
/// Most power commands (1572-1575) in one batch.
pub const MAX_POWER_PER_BATCH: usize = 1;
/// Longest `go_online.for_ms` (10 minutes).
pub const MAX_ONLINE_MS: u32 = 600_000;
/// Longest password of `join` and `host`, in bytes.
const PASSWORD_MAX: usize = 63;
/// Shortest password of `host`, in bytes.
const HOST_PASSWORD_MIN: usize = 8;
/// `wear.flags` bit 0: chosen by the battery.
const WEAR_AUTOMATIC: u32 = 1;
/// `join.flags` bit 0: remember the password (the Wi-Fi plugin's store; this
/// host has none yet and accepts the bit unchanged).
const JOIN_REMEMBER: u32 = 1;

/// When the user last acted and what time it is, injected so that the
/// window rules are testable without a real clock.
pub trait InputClock {
    /// The host clock in milliseconds.
    fn now_ms(&self) -> u64;
    /// When the user last pressed, touched or drew; none yet.
    fn last_input_ms(&self) -> Option<u64>;
}

/// The networks whose password the device keeps (SEC's store). No such store
/// exists in the kernel yet: [`NoStoredNetworks`] says "none stored", so every
/// join needs a recent input or the network manager until it does.
pub trait StoredNetworks {
    /// Whether a password is stored for `ssid`.
    fn has_password(&self, ssid: &str) -> bool;
}

/// No network has a stored password.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoStoredNetworks;

impl StoredNetworks for NoStoredNetworks {
    fn has_password(&self, _ssid: &str) -> bool {
        false
    }
}

/// The kernel's own clock: `Kernel::now` and the time of the last user input.
#[derive(Clone, Copy, Debug)]
pub struct KernelClock {
    now_ms: u64,
    last_input_ms: Option<u64>,
}

impl InputClock for KernelClock {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }
    fn last_input_ms(&self) -> Option<u64> {
        self.last_input_ms
    }
}

/// What the apply step consults besides the kernel: the clock and the store.
#[derive(Clone, Copy)]
pub struct DevHost<'a> {
    /// Time and the last user input.
    pub clock: &'a dyn InputClock,
    /// Stored network passwords.
    pub stored: &'a dyn StoredNetworks,
}

impl fmt::Debug for DevHost<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevHost").finish_non_exhaustive()
    }
}

/// A password: zeroed when dropped, and its `Debug` shows neither text nor
/// length. It is copied into a plain `String` only by [`Secret::to_text`],
/// when the interface-6 radio command is built (that type holds a `String`;
/// its own `Debug` hides the password).
pub(crate) struct Secret(Vec<u8>);

impl Secret {
    fn copy(text: &str) -> Result<Self, i32> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(text.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        bytes.extend_from_slice(text.as_bytes());
        Ok(Self(bytes))
    }

    fn to_text(&self) -> Result<String, i32> {
        let mut text = String::new();
        text.try_reserve_exact(self.0.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        text.push_str(core::str::from_utf8(&self.0).map_err(|_| ERR_INVALID)?);
        Ok(text)
    }
}

impl Secret {
    /// Overwrite the bytes with zeros (best effort: the optimizer may not
    /// elide it past `black_box`).
    fn wipe(&mut self) {
        self.0.iter_mut().for_each(|byte| *byte = 0);
        core::hint::black_box(&self.0);
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// A staged `join` or `host`.
#[derive(Debug)]
pub(crate) struct JoinOp {
    request: u32,
    host: bool,
    ssid: String,
    password: Secret,
}

/// One accepted DEV command, ready to apply.
#[derive(Debug)]
pub(crate) enum DevOp {
    WifiOn,
    WifiOff,
    WifiScan,
    WifiLeave,
    NetworkNone,
    GoOnline {
        for_ms: u32,
    },
    Sleep,
    PowerOff,
    Restart,
    Wear {
        mode: Id,
        automatic: bool,
    },
    Join(JoinOp),
    /// A firmware command; the request was accepted when it was submitted.
    Firmware(FirmwareOp),
}

/// A DEV command and the number of ordinary staged commands before it.
#[derive(Debug)]
pub(crate) struct DevItem {
    at: usize,
    op: DevOp,
}

/// The DEV commands of one callback, in submission order. It owns the
/// cleanup of the request table's reservations: applied by
/// [`Kernel::drain_native_dev`] they become waiting entries, and dropped
/// without that (a refused drain, an error on the way) the ones still
/// reserved are given back, so four dropped batches cannot fill the table.
#[derive(Debug, Default)]
pub(crate) struct DevBatch {
    items: Vec<DevItem>,
    release: Option<Release>,
    /// The firmware requests accepted at submit that no drain applied yet.
    fw: Option<FwRelease>,
}

/// Gives back the reserved (never applied) table entries of one staged batch
/// when dropped. The entries carry the batch's token, so another live batch
/// of the same owner, or a later request that reuses a number, is untouched.
struct Release {
    table: Rc<RefCell<RadioRequests>>,
    token: u32,
}

impl fmt::Debug for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Release").finish_non_exhaustive()
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        if let Ok(mut table) = self.table.try_borrow_mut() {
            table.discard_reserved(self.token);
        }
    }
}

impl DevBatch {
    /// Whether it holds no command to apply.
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// What [`CommandStage::into_dev_output`] returns.
#[cfg(test)]
pub(crate) type DevBatchOutput = (Vec<Command>, Option<ui::Redraw>, u8, DevBatch);

/// What the stage needs to judge DEV commands. Without it every DEV command
/// is `ERR_UNSUPPORTED`, as before the host wired the family.
#[derive(Clone)]
pub(crate) struct DevContext<'a> {
    features: AdmittedFeatures,
    requests: Rc<RefCell<RadioRequests>>,
    guard_owner: Option<&'a str>,
    /// The firmware table and the owner's key; none without a firmware host.
    firmware: Option<FirmwareContext>,
}

impl<'a> DevContext<'a> {
    /// The submitting instance's admitted features and the device-wide table.
    pub(crate) fn new(features: AdmittedFeatures, requests: &Rc<RefCell<RadioRequests>>) -> Self {
        Self {
            features,
            requests: Rc::clone(requests),
            guard_owner: None,
            firmware: None,
        }
    }

    /// The device's firmware table and the submitting plugin's owner key.
    /// Without it every firmware command is `ERR_UNSUPPORTED`.
    pub(crate) fn with_firmware(mut self, firmware: FirmwareContext) -> Self {
        self.firmware = Some(firmware);
        self
    }

    /// The guard's plugin, from the screen stack: while an instance guards only
    /// that plugin may restart or switch off.
    pub(crate) fn guarded_by(mut self, stack: &'a [StackEntry]) -> Self {
        self.guard_owner = stack
            .iter()
            .rev()
            .find(|entry| entry.guards)
            .map(|entry| entry.screen.namespace());
        self
    }
}

/// The stage's DEV state. Dropping it with items still reserved gives their
/// table entries back.
pub(super) struct DevStage<'a> {
    ctx: DevContext<'a>,
    items: Vec<DevItem>,
    /// The token its table entries carry; 0 until it reserves the first.
    token: u32,
    /// The firmware requests this stage accepted (given back when it drops).
    fw: Option<FwRelease>,
    /// The box its batch will wait in for the kernel, made with the first
    /// DEV command: that command answers `ERR_NO_MEMORY` where it cannot be,
    /// and a batch answered `OK` needs nothing more to be kept.
    boxed: Option<Box<DevBatch>>,
}

impl DevStage<'_> {
    fn release_reserved(&self) {
        if let Ok(mut table) = self.ctx.requests.try_borrow_mut() {
            table.discard_reserved(self.token);
        }
    }
}

impl Drop for DevStage<'_> {
    fn drop(&mut self) {
        self.release_reserved();
    }
}

impl fmt::Debug for DevStage<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevStage")
            .field("items", &self.items)
            .finish_non_exhaustive()
    }
}

/// Whether this wire command belongs to the Wi-Fi or power area.
pub(super) fn is_dev(command: &WireCommand<'_>) -> bool {
    area(command).is_some()
}

fn area(command: &WireCommand<'_>) -> Option<&'static str> {
    match command {
        WireCommand::WifiOn { .. }
        | WireCommand::WifiOff { .. }
        | WireCommand::WifiScan { .. }
        | WireCommand::WifiJoin { .. }
        | WireCommand::WifiLeave { .. }
        | WireCommand::WifiHost { .. }
        | WireCommand::NetworkNone { .. }
        | WireCommand::GoOnline { .. } => Some(FEATURE_WIFI),
        WireCommand::Sleep { .. }
        | WireCommand::PowerOff { .. }
        | WireCommand::Restart { .. }
        | WireCommand::Wear { .. } => Some(FEATURE_POWER),
        _ => firmware::area(command),
    }
}

impl<'a> CommandStage<'a> {
    /// Let this stage judge and keep DEV commands (the shell passes the
    /// submitting instance's features and the kernel's table). The stage
    /// gives back its own reservations when dropped; it never touches those
    /// of another stage.
    #[cfg(test)]
    pub(crate) fn with_dev(mut self, ctx: DevContext<'a>) -> Self {
        self.admit_dev(ctx);
        self
    }

    /// As [`Self::with_dev`], on a stage already lent (the instance's
    /// features are known only inside its callback scope). While no guard
    /// was named, the guard is read from the stage's own stack.
    pub(crate) fn admit_dev(&mut self, mut ctx: DevContext<'a>) {
        if ctx.guard_owner.is_none()
            && let Some(ui) = &self.ui
        {
            ctx = ctx.guarded_by(ui.env().stack());
        }
        self.dev = Some(DevStage {
            ctx,
            items: Vec::new(),
            token: 0,
            fw: None,
            boxed: None,
        });
    }

    /// Like `into_ui_output`, with the DEV commands to apply in order. The
    /// reservations pass to the returned batch, which gives back the ones it
    /// never applied when dropped.
    #[cfg(test)]
    pub(crate) fn into_dev_output(mut self) -> (Vec<Command>, Option<ui::Redraw>, u8, DevBatch) {
        let batch = self.take_dev();
        let (commands, redraw, hop) = self.into_ui_output();
        (commands, redraw, hop, batch)
    }

    /// [`Self::take_dev`] for a batch that waits for the kernel: in the box
    /// made with its first command (nothing is allocated here), and only when
    /// it holds a command, so that a waiting batch without one costs a
    /// pointer.
    pub(crate) fn take_dev_boxed(&mut self) -> Option<Box<DevBatch>> {
        let boxed = self.dev.as_mut().and_then(|dev| dev.boxed.take());
        let batch = self.take_dev();
        if batch.is_empty() {
            return None;
        }
        // A kept command was staged after `dev_reserve` made the box.
        let mut boxed = boxed?;
        *boxed = batch;
        Some(boxed)
    }

    /// The DEV commands to apply in order (call it before the stage is
    /// turned into its output). The reservations pass to the batch, which
    /// gives back the ones it never applied when dropped.
    pub(crate) fn take_dev(&mut self) -> DevBatch {
        let Some(dev) = self.dev.as_mut() else {
            return DevBatch::default();
        };
        DevBatch {
            items: core::mem::take(&mut dev.items),
            // The token moves to the batch; the stage must not give back
            // what the batch now owns.
            release: Some(Release {
                table: Rc::clone(&dev.ctx.requests),
                token: core::mem::take(&mut dev.token),
            }),
            fw: dev.fw.take(),
        }
    }

    /// Check the DEV commands of a batch without keeping anything; the bytes
    /// they will copy.
    #[inline(never)]
    pub(super) fn validate_dev(&self, batch: &CommandBatch<'_>) -> Result<usize, i32> {
        let Some(dev) = &self.dev else {
            return if batch.iter().any(|command| is_dev(&command)) {
                Err(ERR_UNSUPPORTED)
            } else {
                Ok(0)
            };
        };
        // Features first: a missing one refuses the whole batch, valid
        // commands included.
        for key in batch.iter().filter_map(|command| area(&command)) {
            dev.ctx.features.feature(key)?;
        }
        let (mut wifi, mut power, mut bytes) = (0_usize, 0_usize, 0_usize);
        let mut firmware = 0_usize;
        let mut revoked = 0_usize;
        let mut evidence = firmware::EvidenceSeen::default();
        let mut requests = [0_u32; MAX_WIFI_PER_BATCH];
        for command in batch.iter() {
            if firmware::is_evidence(&command) {
                // Not a command of its own: it belongs to the install before it.
                evidence.take(&command)?;
                bytes = bytes.saturating_add(self.validate_firmware(&command)?);
                continue;
            }
            if matches!(command, WireCommand::TrustRevoked { .. }) {
                // Its own count; it stands between an install and its
                // evidence like any other command.
                evidence.other_command();
                revoked += 1;
                if revoked > firmware::MAX_REVOKED_PER_BATCH {
                    return Err(ERR_INVALID);
                }
                bytes = bytes.saturating_add(self.validate_firmware(&command)?);
                continue;
            }
            if let WireCommand::InstallPlugin { request, .. } = command {
                evidence.install(request);
            } else if area(&command).is_some() {
                evidence.other_command();
            }
            if firmware::is_firmware(&command) {
                firmware += 1;
            } else {
                match area(&command) {
                    Some(FEATURE_WIFI) => wifi += 1,
                    Some(_) => power += 1,
                    None => continue,
                }
            }
            if wifi > MAX_WIFI_PER_BATCH
                || power > MAX_POWER_PER_BATCH
                || firmware > firmware::MAX_PER_BATCH
            {
                return Err(ERR_INVALID);
            }
            bytes = bytes.saturating_add(self.validate_dev_command(&command, &mut requests)?);
        }
        Ok(bytes)
    }

    fn validate_dev_command(
        &self,
        command: &WireCommand<'_>,
        seen: &mut [u32; MAX_WIFI_PER_BATCH],
    ) -> Result<usize, i32> {
        match *command {
            WireCommand::WifiJoin {
                request,
                ssid,
                password,
                flags,
            } => {
                // Bit 0 (remember) is the only bit defined; another is a
                // contract this host does not know, refused, not ignored.
                if flags & !JOIN_REMEMBER != 0 {
                    return Err(ERR_UNSUPPORTED);
                }
                check_request(request, ssid, password.len(), 0, seen)?;
                Ok(ssid.len() + password.len())
            }
            WireCommand::WifiHost {
                request,
                ssid,
                password,
            } => {
                check_request(request, ssid, password.len(), HOST_PASSWORD_MIN, seen)?;
                Ok(ssid.len() + password.len())
            }
            WireCommand::GoOnline { for_ms } if !(1..=MAX_ONLINE_MS).contains(&for_ms) => {
                Err(ERR_INVALID)
            }
            WireCommand::Wear { mode, flags } => {
                if flags & !WEAR_AUTOMATIC != 0 {
                    return Err(ERR_UNSUPPORTED);
                }
                let known = self
                    .settings
                    .presets()
                    .get(mode)
                    .is_some_and(|preset| preset.kind == MODE_KIND);
                if known {
                    Ok(mode.len())
                } else {
                    Err(ERR_NOT_FOUND)
                }
            }
            WireCommand::PowerOff { .. } | WireCommand::Restart { .. } => {
                self.guard_allows().map(|()| 0)
            }
            _ => self.validate_firmware(command),
        }
    }

    /// While an instance guards, only the guard's plugin may restart or
    /// switch off (`ERR_DENIED`); sleep stays open.
    fn guard_allows(&self) -> Result<(), i32> {
        match self.dev.as_ref().and_then(|dev| dev.ctx.guard_owner) {
            Some(guard) if guard != self.owner.as_str() => Err(ERR_DENIED),
            _ => Ok(()),
        }
    }

    /// Room for this batch's DEV items, before any is pushed.
    #[inline(never)]
    pub(super) fn dev_reserve(&mut self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let count = batch.iter().filter(|command| is_dev(command)).count();
        match &mut self.dev {
            Some(dev) if count > 0 => {
                if dev.boxed.is_none() {
                    let made = crate::native_host::shared::try_box(DevBatch::default());
                    dev.boxed = Some(made.ok_or(ERR_NO_MEMORY)?);
                }
                dev.items.try_reserve(count).map_err(|_| ERR_NO_MEMORY)
            }
            _ => Ok(()),
        }
    }

    /// Keep one validated DEV command after the ordinary ones before it.
    #[inline(never)]
    pub(super) fn dev_push(&mut self, command: &WireCommand<'_>) -> Result<(), i32> {
        let at = self.staged.len();
        if firmware::is_evidence(command) {
            return self.firmware_evidence(command);
        }
        let op = match *command {
            WireCommand::WifiOn { .. } => DevOp::WifiOn,
            WireCommand::WifiOff { .. } => DevOp::WifiOff,
            WireCommand::WifiScan { .. } => DevOp::WifiScan,
            WireCommand::WifiLeave { .. } => DevOp::WifiLeave,
            WireCommand::NetworkNone { .. } => DevOp::NetworkNone,
            WireCommand::GoOnline { for_ms } => DevOp::GoOnline { for_ms },
            WireCommand::Sleep { .. } => DevOp::Sleep,
            WireCommand::PowerOff { .. } => DevOp::PowerOff,
            WireCommand::Restart { .. } => DevOp::Restart,
            WireCommand::Wear { mode, flags } => DevOp::Wear {
                mode: ui::copy_id(mode)?,
                automatic: flags & WEAR_AUTOMATIC != 0,
            },
            WireCommand::WifiJoin {
                request,
                ssid,
                password,
                ..
            } => join_op(request, false, ssid, password.expose())?,
            WireCommand::WifiHost {
                request,
                ssid,
                password,
            } => join_op(request, true, ssid, password.expose())?,
            _ => self.firmware_op(command)?,
        };
        let dev = self.dev.as_mut().ok_or(ERR_UNSUPPORTED)?;
        // Capacity was reserved by `dev_reserve`; this cannot reallocate.
        dev.items.push(DevItem { at, op });
        Ok(())
    }

    /// Reserve the table entries of the items from `start`, all or none.
    /// Its own frame: a cold path (DEV batches only) whose table borrows and
    /// requests are not held on `push_batch`'s (the S3 frame limit).
    #[inline(never)]
    pub(super) fn dev_commit(&mut self, start: usize) -> Result<(), i32> {
        self.dev_commit_radio(start)?;
        if let Err(error) = self.firmware_commit(start) {
            self.dev_release_radio(start);
            return Err(error);
        }
        Ok(())
    }

    /// Gives back the table entries of the Wi-Fi items from `start`.
    fn dev_release_radio(&mut self, start: usize) {
        let Some(dev) = &self.dev else {
            return;
        };
        if let Ok(mut table) = dev.ctx.requests.try_borrow_mut() {
            for item in dev.items.get(start..).unwrap_or_default() {
                if let DevOp::Join(join) = &item.op {
                    table.release_reserved(self.owner.as_str(), join.request);
                }
            }
        }
    }

    fn dev_commit_radio(&mut self, start: usize) -> Result<(), i32> {
        let Some(dev) = &mut self.dev else {
            return Ok(());
        };
        let requests = Rc::clone(&dev.ctx.requests);
        let mut table = requests.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let new = dev.items.get(start..).unwrap_or_default();
        // A token only for a batch that reserves something, and before the
        // first reservation: its exhaustion refuses the batch whole.
        if dev.token == 0 && new.iter().any(|item| matches!(item.op, DevOp::Join(_))) {
            dev.token = table.new_token().ok_or(ERR_FULL)?;
        }
        for (index, item) in new.iter().enumerate() {
            let DevOp::Join(join) = &item.op else {
                continue;
            };
            let reserved =
                table.reserve_in(dev.token, self.owner, join.request, join.host, &join.ssid);
            if let Err(error) = reserved {
                for earlier in new.iter().take(index) {
                    if let DevOp::Join(done) = &earlier.op {
                        table.release_reserved(self.owner.as_str(), done.request);
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    }

    /// Drop the items from `start` (their table entries are already given back
    /// by a failed `dev_commit`, or were never reserved).
    pub(super) fn dev_rollback(&mut self, start: usize) {
        if let Some(dev) = &mut self.dev {
            dev.items.truncate(start);
        }
    }

    /// How many DEV items are kept (the rollback mark).
    pub(super) fn dev_len(&self) -> usize {
        self.dev.as_ref().map_or(0, |dev| dev.items.len())
    }
}

fn check_request(
    request: u32,
    ssid: &str,
    password_len: usize,
    password_min: usize,
    seen: &mut [u32; MAX_WIFI_PER_BATCH],
) -> Result<(), i32> {
    if request == 0
        || ssid.is_empty()
        || password_len > PASSWORD_MAX
        || password_len < password_min
        || seen.contains(&request)
    {
        return Err(ERR_INVALID);
    }
    if let Some(slot) = seen.iter_mut().find(|slot| **slot == 0) {
        *slot = request;
    }
    Ok(())
}

fn join_op(request: u32, host: bool, ssid: &str, password: &str) -> Result<DevOp, i32> {
    let mut copy = String::new();
    copy.try_reserve_exact(ssid.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(ssid);
    Ok(DevOp::Join(JoinOp {
        request,
        host,
        ssid: copy,
        password: Secret::copy(password)?,
    }))
}

impl crate::Kernel {
    /// The clock of the window rules: the kernel's time and its last user input.
    pub(crate) fn dev_clock(&self) -> KernelClock {
        KernelClock {
            now_ms: self.now.millis(),
            last_input_ms: self.last_user_input.map(paperpaw_platform::Instant::millis),
        }
    }

    /// The sender stopped: its radio requests are released, nothing delivered.
    pub(crate) fn radio_sender_stopped(&self, owner: &str) {
        if let Ok(mut table) = self.radio_requests.try_borrow_mut() {
            table.sender_stopped(owner);
        }
    }

    /// Like `drain_native_ui`, applying the owner's DEV commands in batch
    /// order among the ordinary ones, as interface-6 commands. A power command
    /// is applied after every command before it in its batch; the shell acts
    /// on the kernel's sleep, power-off and restart flags once the frame is
    /// shown (`Effects`).
    #[cfg(test)]
    pub(crate) fn drain_native_dev(
        &mut self,
        owner: &Id,
        commands: Vec<Command>,
        redraw: Option<ui::Redraw>,
        batch: DevBatch,
        host: &DevHost<'_>,
    ) {
        let opens = super::Opens::default();
        self.drain_native_batch(owner, (commands, redraw, opens), batch, host);
    }

    /// Applies the DEV items of `batch` whose place is at or before the
    /// ordinary command `index` (`None`: all that are left).
    pub(crate) fn apply_dev_until(
        &mut self,
        owner: &Id,
        batch: &mut DevBatch,
        index: Option<usize>,
        host: &DevHost<'_>,
    ) {
        let token = batch.release.as_ref().map_or(0, |release| release.token);
        let due = batch
            .items
            .iter()
            .take_while(|item| index.is_none_or(|index| item.at <= index))
            .count();
        let mut fw = batch.fw.take();
        for item in batch.items.drain(..due) {
            self.apply_dev(owner, item.op, host, token, &mut fw);
        }
        batch.fw = fw;
    }

    fn apply_dev(
        &mut self,
        owner: &Id,
        op: DevOp,
        host: &DevHost<'_>,
        token: u32,
        fw: &mut Option<FwRelease>,
    ) {
        match op {
            DevOp::WifiOn => self.apply(Command::Wifi(WifiCommand::On)),
            DevOp::WifiScan => self.apply(Command::Wifi(WifiCommand::Scan)),
            DevOp::WifiOff => self.radio_stops(WifiCommand::Off),
            DevOp::WifiLeave => self.radio_stops(WifiCommand::Leave),
            DevOp::NetworkNone => self.apply(Command::NoNetwork),
            DevOp::GoOnline { for_ms } => self.apply_go_online(for_ms, host),
            DevOp::Sleep => self.apply(Command::Sleep),
            // A guard that came up since the submit holds them too.
            DevOp::PowerOff | DevOp::Restart if self.guarded_against(owner) => {
                self.warn_runtime("native power-off or restart dropped: another plugin guards");
            }
            DevOp::PowerOff => self.apply(Command::PowerOff),
            DevOp::Restart => self.apply(Command::Restart),
            DevOp::Wear { mode, automatic } => {
                let wear = Wear::new(mode);
                self.apply(Command::Wear(if automatic {
                    wear.automatic()
                } else {
                    wear
                }));
            }
            DevOp::Join(join) => self.apply_join(owner, join, host, token),
            DevOp::Firmware(op) => Self::apply_firmware(op, fw),
        }
    }

    /// Whether a screen of another plugin than `owner` guards the device now.
    fn guarded_against(&self, owner: &Id) -> bool {
        self.guard()
            .and_then(|guard| self.stack.get(guard))
            .is_some_and(|guard| guard.id.namespace() != owner.as_str())
    }

    fn radio_stops(&mut self, command: WifiCommand) {
        if let Ok(mut table) = self.radio_requests.try_borrow_mut() {
            table.radio_stopped();
        }
        self.apply(Command::Wifi(command));
    }

    /// `go_online` is honoured only within the window after a user input,
    /// else it is ignored with a log line (no ssid, no password in it).
    fn apply_go_online(&mut self, for_ms: u32, host: &DevHost<'_>) {
        if !input_is_recent(host.clock) {
            self.warn_runtime("go_online ignored: no user input in the last 2 seconds");
            return;
        }
        // The host judged this the user's choice; the kernel's own rule asks
        // for the reader's call, which a later event in the window is not.
        let reader = core::mem::replace(&mut self.shared.reader, true);
        self.apply(Command::GoOnlineFor(Duration::from_millis(u64::from(
            for_ms,
        ))));
        self.shared.reader = reader;
    }

    /// The plugin the user chose as network manager (`core.network-manager`).
    pub(crate) fn network_manager(&self) -> Option<&str> {
        match self.shared.settings.global(NETWORK_MANAGER) {
            Some(SettingValue::Text(text)) if !text.is_empty() => Some(&**text),
            _ => None,
        }
    }

    /// Whether `owner` is the plugin the user chose as network manager.
    fn is_network_manager(&self, owner: &Id) -> bool {
        self.network_manager() == Some(owner.as_str())
    }

    fn apply_join(&mut self, owner: &Id, join: JoinOp, host: &DevHost<'_>, token: u32) {
        let (name, request) = (owner.as_str(), join.request);
        // Only the batch that reserved the request may apply it: an output
        // that outlived its sender (or whose number a newer request reuses)
        // sends nothing and touches no entry.
        let live = self
            .radio_requests
            .try_borrow()
            .is_ok_and(|table| table.holds_reserved_in(token, name, request));
        if !live {
            return;
        }
        let end = |kernel: &Self, reason: u32| {
            if let Ok(mut table) = kernel.radio_requests.try_borrow_mut() {
                table.end(name, request, Outcome::failed(reason));
            }
        };
        if self.shared.wifi.is_none() {
            return end(self, reason::UNSUPPORTED);
        }
        // No silent network change: a stored password, a recent input or the
        // network manager. A host never has a stored password.
        let known = !join.host && host.stored.has_password(&join.ssid);
        if !(known || input_is_recent(host.clock) || self.is_network_manager(owner)) {
            return end(self, reason::NEEDS_USER);
        }
        let Ok(password) = join.password.to_text() else {
            return end(self, reason::OTHER);
        };
        if let Ok(mut table) = self.radio_requests.try_borrow_mut() {
            table.supersede_waiting();
            table.activate(name, request, host.clock.now_ms());
        }
        let ssid = join.ssid;
        self.apply(Command::Wifi(if join.host {
            WifiCommand::Host { ssid, password }
        } else {
            WifiCommand::Join { ssid, password }
        }));
    }
}

fn input_is_recent(clock: &dyn InputClock) -> bool {
    clock
        .last_input_ms()
        .is_some_and(|last| clock.now_ms().saturating_sub(last) <= INPUT_WINDOW_MS)
}

#[cfg(test)]
#[path = "commands_dev_tests.rs"]
mod tests;
