### Added

- **Dual boot for native plugins** (#228): `dev_firmware.flags` says whether the setup is declared dual-boot (bit 0) and whether `paperpaw.ini` says `dual-boot = off` (bit 1). `command.firmware_dual_boot` (1595) turns the check off, always confirmed by the host and followed by a restart, or back to `auto`, which takes effect at the next start (`docs/abi/device.md` 5.3), for the update plugin's interface-6 dual-boot notice and button.

### Changed

- **`paperpaw.ini` is closed to plugins on root 1** (#228), like `plugins/` and `.paperpaw/`: its `[firmware]` section guards a dual-boot setup, so only the host's confirmed command changes it (`docs/abi/files.md` 1.1).
