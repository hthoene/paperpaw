### Changed

- **Emulator scripts can take the card out** (#244). The steps `card-out` and `card-in` of `--script`: while the card is out, everything asked of the storage fails as on a card that went. For tests only, `Kernel::show_firmware_confirm` (feature `testing` of `paperpaw-core`, never in a firmware) shows the firmware confirmation without a firmware host.
