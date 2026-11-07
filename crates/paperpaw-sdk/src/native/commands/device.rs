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
}
