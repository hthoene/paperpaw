# Embassy USB driver for the Synopsys USB OTG core

> Modified for PaperPaw: embassy-usb-synopsys-otg 0.4.0 as published, with
> one change in `src/lib.rs` (`State::reset`, called by `Driver::new`), a
> backport of embassy commit 4371e1d4c5 (not yet released): a driver made
> after an earlier one on the same peripheral starts from no allocated
> endpoints. Without it the X4 Pro's and X4 Classic's second USB drive in a
> run panicked in `Driver::start` (its control endpoints got index 3, not 0).
> Licensed as the original (`LICENSE-APACHE`, `LICENSE-MIT`).

This crate implements [`embassy-usb-driver`](https://crates.io/crates/embassy-usb-driver) for Synopsys USB OTG devices.

It contains the "core" of the driver that is common across all chips using
the Synopsys OTG IP, but it doesn't contain chip-specific initialization such
as clock setup and GPIO muxing. You most likely don't want to use this crate
directly, but use it through a HAL that does the initialization for you.

List of HALs integrating this driver:

- [`embassy-stm32`](https://crates.io/crates/embassy-stm32), for STMicroelectronics STM32 chips.
- [`esp-hal`](https://crates.io/crates/esp-hal), for Espressif ESP32 chips.

If you wish to integrate this crate into your device's HAL, you will need to add the
device-specific initialization. See the above crates for examples on how to do it.