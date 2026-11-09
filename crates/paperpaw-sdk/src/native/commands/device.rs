//! Device commands: radio, network policy and power (`docs/abi/device.md` 3).
//!
//! A plugin declares `uses.wifi` for the radio commands and `uses.power` for
//! the power ones; the host refuses a batch of a command whose feature is
//! not declared. Commands apply after the callback, in order. They carry no
//! reply: the outcome is an event (`wifi_done`, the power events) and the
//! state is read again from `core.device`.
use super::{CommandSender, Error, wire};
use crate::native::ui::Open;

/// What a power command does after the screen before it in its batch has drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PowerAction {
    /// Suspend the device.
    Sleep,
    /// Switch the device off, where it can.
    PowerOff,
    /// Restart the device after plugins have saved what they keep.
    Restart,
}

impl PowerAction {
    const fn command(self) -> wire::Command<'static> {
        match self {
            Self::Sleep => wire::Command::Sleep {},
            Self::PowerOff => wire::Command::PowerOff {},
            Self::Restart => wire::Command::Restart {},
        }
    }
}

/// Where the plugin or firmware image to install is, in the folder the host
/// reads it from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallRoot {
    /// This plugin's data folder (`core.net` `save_to` root 0 puts a download there).
    Data,
    /// The user's files on the card (needs `uses.storage`).
    Files,
}

impl InstallRoot {
    const fn wire(self) -> u32 {
        match self {
            Self::Data => 0,
            Self::Files => 1,
        }
    }
}

/// How a firmware image is installed: the options of
/// [`CommandSender::install_firmware`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FirmwareOptions<'a> {
    allow_older: bool,
    foreign: bool,
    key: Option<&'a [u8; 32]>,
}

impl<'a> FirmwareOptions<'a> {
    /// A PaperPaw image, newer than the running one, signed by a PaperPaw key.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            allow_older: false,
            foreign: false,
            key: None,
        }
    }

    /// Also take an older or the same version.
    #[must_use]
    pub const fn allow_older(mut self) -> Self {
        self.allow_older = true;
        self
    }

    /// The image is another firmware than PaperPaw: unsigned, checked as the boot
    /// loader would start it. The host always asks, and tells the user that
    /// PaperPaw does not start again by itself.
    #[must_use]
    pub const fn foreign(mut self) -> Self {
        self.foreign = true;
        self
    }

    /// The signing key (32 bytes) of an image signed by another key than
    /// PaperPaw's, which installs only when this names exactly that key and the
    /// user accepts it in the host's confirmation. A foreign image is not
    /// signed, so a key has no meaning with [`foreign`](Self::foreign): the
    /// contract states no combination of the two, and the host decides.
    #[must_use]
    pub const fn key(mut self, key: &'a [u8; 32]) -> Self {
        self.key = Some(key);
        self
    }
}

impl CommandSender<'_, '_> {
    /// Turn the radio on (`uses.wifi`). Ignored where the device has none.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal (`Denied` without `uses.wifi`).
    pub fn wifi_on(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::WifiOn {})
    }

    /// Turn the radio off (`uses.wifi`).
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn wifi_off(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::WifiOff {})
    }

    /// Scan for networks (`uses.wifi`); the result is `core.device` op 9.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn wifi_scan(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::WifiScan {})
    }

    /// Join a network (`uses.wifi`). `request` is the caller's own non-zero
    /// number, answered by `wifi_done`; `remember` keeps the password.
    ///
    /// # Errors
    /// Returns `Invalid` for a zero request, an empty or overlong name, or a
    /// password above 63 bytes; buffer limits or the host's refusal otherwise.
    pub fn wifi_join(
        &mut self,
        request: u32,
        ssid: &str,
        password: &str,
        remember: bool,
    ) -> Result<(), Error> {
        if request == 0 || ssid.is_empty() {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::WifiJoin {
            request,
            ssid,
            password: wire::SecretText::new(password),
            flags: u32::from(remember),
        })
    }

    /// Leave the network (`uses.wifi`).
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn wifi_leave(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::WifiLeave {})
    }

    /// Host a network (`uses.wifi`); its password has 8 to 63 bytes.
    ///
    /// # Errors
    /// Returns `Invalid` for a zero request, an empty or overlong name, or a
    /// password outside 8..=63 bytes; buffer limits or the host's refusal.
    pub fn wifi_host(&mut self, request: u32, ssid: &str, password: &str) -> Result<(), Error> {
        if request == 0 || ssid.is_empty() || !(8..=63).contains(&password.len()) {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::WifiHost {
            request,
            ssid,
            password: wire::SecretText::new(password),
        })
    }

    /// Say that no network is wanted (`uses.wifi`).
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn network_none(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::NetworkNone {})
    }

    /// Ask for the network for `for_ms` (1 to 600,000), within the host's input
    /// window (`uses.wifi`).
    ///
    /// # Errors
    /// Returns `Invalid` outside the range, buffer limits or the host's refusal.
    pub fn go_online(&mut self, for_ms: u32) -> Result<(), Error> {
        if !(1..=600_000).contains(&for_ms) {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::GoOnline { for_ms })
    }

    /// Suspend the device (`uses.power`), after the frame being drawn.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn sleep(&mut self) -> Result<(), Error> {
        self.submit(PowerAction::Sleep.command())
    }

    /// Switch the device off (`uses.power`). Ignored where PaperPaw cannot.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal (`Denied` while another
    /// plugin's screen guards).
    pub fn power_off(&mut self) -> Result<(), Error> {
        self.submit(PowerAction::PowerOff.command())
    }

    /// Restart the device (`uses.power`).
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn restart(&mut self) -> Result<(), Error> {
        self.submit(PowerAction::Restart.command())
    }

    /// Open a screen or slot and then act on power in the same batch
    /// (`uses.power`): the host draws the screen first and acts when the
    /// panel is idle, so the e-ink shows the device's real state. If the
    /// screen fails to open, the power command still acts.
    ///
    /// # Errors
    /// Returns the navigation's invalid ids, buffer limits or the host's refusal.
    pub fn show_then(&mut self, open: Open<'_>, action: PowerAction) -> Result<(), Error> {
        self.submit_batch(&[open.command()?, action.command()])
    }

    /// Wear a power mode (`uses.power`): a registry preset id of kind
    /// `core.power-mode`. `automatic` says the battery chose it.
    ///
    /// # Errors
    /// Returns `Invalid` for an empty mode, buffer limits or the host's
    /// refusal (`NotFound` for an unknown preset).
    pub fn wear(&mut self, mode: &str, automatic: bool) -> Result<(), Error> {
        if mode.is_empty() {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::Wear {
            mode,
            flags: u32::from(automatic),
        })
    }

    /// Ask the host to install the plugin downloaded into `path` of `root`
    /// (`uses.install`, `docs/abi/device.md` 5.2): a `.ppn` file or a plugin
    /// folder. The host copies it, checks it and **asks the user** before it
    /// installs (decision 132); a plugin never decides. `update`
    /// replaces an installed plugin of the same id, `allow_older` also takes an
    /// older version. `request` is the caller's own non-zero number: the end of
    /// the request is `install_done` (kind 0) and `Device::install` its state.
    /// A user who granted this plugin the trusted-installer grant is not asked
    /// (the checks still run). The catalog's author key, recovery key and
    /// rotations (`install_author`, `install_rotation`, commands 1580 and 1581)
    /// are not sent by this method.
    ///
    /// # Errors
    /// Returns `Invalid` for a zero request or an empty path, buffer limits or
    /// the host's refusal (`Denied` without `uses.install`, `Full` when four
    /// install, removal, firmware or check requests wait already).
    pub fn install_plugin(
        &mut self,
        request: u32,
        root: InstallRoot,
        path: &str,
        update: bool,
        allow_older: bool,
    ) -> Result<(), Error> {
        if request == 0 || path.is_empty() {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::InstallPlugin {
            request,
            root: root.wire(),
            path,
            flags: u32::from(update) | (u32::from(allow_older) << 1),
        })
    }

    /// Ask the host to remove the installed plugin `plugin` (`uses.install`,
    /// `docs/abi/device.md` 5.2): the user is asked once, with the choice to
    /// keep the plugin's data (decision 124). `request` is the caller's own
    /// non-zero number: the end is `install_done` (kind 2).
    ///
    /// # Errors
    /// Returns `Invalid` for a zero request or an empty id, buffer limits or
    /// the host's refusal (`Denied` without `uses.install`).
    pub fn remove_plugin(&mut self, request: u32, plugin: &str) -> Result<(), Error> {
        if request == 0 || plugin.is_empty() {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::RemovePlugin {
            request,
            plugin,
            flags: 0,
        })
    }

    /// Ask the host to install the firmware image in `path` of `root`
    /// (`uses.firmware`, `docs/abi/device.md` 5.3); `root` [`InstallRoot::Files`]
    /// also needs `uses.storage`. The host checks the image, **always asks the
    /// user** (whatever grant the plugin has) and writes the other flash slot
    /// in bounded steps. `request` is the caller's own non-zero number: the end
    /// is `install_done` (kind 1) and `Device::install` its state. Installing
    /// does not restart: [`firmware_restart`](Self::firmware_restart) starts
    /// the installed image.
    ///
    /// # Errors
    /// Returns `Invalid` for a zero request or an empty path, `NoSpace` for a
    /// path over 512 bytes or buffer limits, or the host's refusal (`Denied`
    /// without `uses.firmware`, `Full` when four install, removal, firmware or
    /// check requests wait already).
    pub fn install_firmware(
        &mut self,
        request: u32,
        root: InstallRoot,
        path: &str,
        options: FirmwareOptions<'_>,
    ) -> Result<(), Error> {
        if request == 0 || path.is_empty() {
            return Err(Error::Invalid);
        }
        if path.len() > 512 {
            return Err(Error::NoSpace);
        }
        self.submit(wire::Command::InstallFirmware {
            request,
            root: root.wire(),
            path,
            key: options.key.map_or(&[][..], |key| &key[..]),
            flags: u32::from(options.allow_older) | (u32::from(options.foreign) << 1),
        })
    }

    /// Start the firmware image installed before (`uses.firmware`); the host
    /// asks nothing more, the user said yes at the install.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal (`Denied` without
    /// `uses.firmware`).
    pub fn firmware_restart(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::FirmwareRestart {})
    }
}
