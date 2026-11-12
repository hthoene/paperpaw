//! The emulator's firmware host: there is no firmware to install here (the
//! computer runs the emulator, not an image), but plugin jobs (a store's
//! install, a removal, a check: `device.md` 5.1, 5.2) share the firmware's
//! queue in the kernel, so the emulated device has one. Every firmware job is
//! refused as on a device shared with another firmware (`Reason::NOT_HERE`,
//! room 0); a plugin job never calls it.
use paperpaw_platform::Storage;
use paperpaw_platform::firmware::{CheckFacts, FirmwareHost, Reason, Running, Step};

/// The chip an app image's header names (`Running::chip`).
pub const CHIP_ESP32_C3: u16 = 5;
pub const CHIP_ESP32_S3: u16 = 9;

/// No firmware is installed here; the emulated device's chip is told.
#[derive(Debug, Clone, Copy)]
pub struct NoFirmware {
    chip: u16,
}

impl NoFirmware {
    pub const fn new(chip: u16) -> Self {
        Self { chip }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_firmware_job_is_not_here_and_the_chip_is_told() {
        let mut host = NoFirmware::new(CHIP_ESP32_C3);
        let running = host.running();
        assert_eq!((running.chip, running.room), (CHIP_ESP32_C3, 0));
        assert!(host.begin_check("update.bin").is_err());
        assert!(
            host.begin_write("update.bin", [0; 32], None, false)
                .is_err()
        );
        assert!(matches!(
            host.write_step(&paperpaw_core::sdk::MemoryStorage::new()),
            Step::Failed(Reason::NOT_HERE)
        ));
        assert!(host.commit().is_err() && host.restart().is_err());
        assert!(!host.dual_boot_capable());
    }
}
