//! Bluetooth commands of the device service (`docs/abi/device.md`,
//! commands 1583-1591). Every one needs `uses.bluetooth` in the signed
//! metadata; a command whose feature is not declared refuses the whole batch.
//! Devices are named by the host's device ids as the reads give them
//! (`BondedDevice::device`); none of the senders waits: the answers come as
//! `event.bluetooth` (98) to the plugin bound to `core.bluetooth`.
use super::{CommandSender, Error, check_id, wire};

impl CommandSender<'_, '_> {
    /// Scan for devices for `lasting_ms` milliseconds (`command.bt_scan`).
    ///
    /// # Errors
    /// Returns the host's refusal, including where `uses.bluetooth` is not declared (the batch is
    /// refused whole) or the device has no radio.
    pub fn bt_scan(&mut self, lasting_ms: u32) -> Result<(), Error> {
        self.submit(wire::Command::BtScan { lasting_ms })
    }

    /// Stop a running scan (`command.bt_stop_scan`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_stop_scan(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::BtStopScan {})
    }

    /// Pair with a device the scan found; the host asks the reader to
    /// confirm (`command.bt_pair`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_pair(&mut self, device: u32) -> Result<(), Error> {
        self.submit(wire::Command::BtPair { device })
    }

    /// Connect a bonded device (`command.bt_connect`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_connect(&mut self, device: u32) -> Result<(), Error> {
        self.submit(wire::Command::BtConnect { device })
    }

    /// End the link to a device; the bond stays (`command.bt_disconnect`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_disconnect(&mut self, device: u32) -> Result<(), Error> {
        self.submit(wire::Command::BtDisconnect { device })
    }

    /// Forget a bonded device and its keys (`command.bt_forget`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_forget(&mut self, device: u32) -> Result<(), Error> {
        self.submit(wire::Command::BtForget { device })
    }

    /// Give a bonded sensor to `plugin`, which then uses it (`command.bt_give`).
    ///
    /// # Errors
    /// Returns Invalid for an empty or over-long plugin id, or the host's
    /// refusal.
    pub fn bt_give(&mut self, device: u32, plugin: impl AsRef<str>) -> Result<(), Error> {
        let plugin = plugin.as_ref();
        check_id(plugin, wire::COMMAND_BT_GIVE_PLUGIN_MAX_BYTES)?;
        self.submit(wire::Command::BtGive { device, plugin })
    }

    /// Take a given sensor back from the plugin it was given to
    /// (`command.bt_take_back`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_take_back(&mut self, device: u32) -> Result<(), Error> {
        self.submit(wire::Command::BtTakeBack { device })
    }

    /// Send a remote's or a keyboard's keys to the key maps, or stop
    /// (`command.bt_keys`).
    ///
    /// # Errors
    /// Returns the host's refusal.
    pub fn bt_keys(&mut self, device: u32, on: bool) -> Result<(), Error> {
        self.submit(wire::Command::BtKeys {
            device,
            on: u32::from(on),
        })
    }
}
