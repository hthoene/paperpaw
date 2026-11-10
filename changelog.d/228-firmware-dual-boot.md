### Added

- **Dual boot for native plugins** (#228): `dev_firmware.flags` says whether the setup is declared dual-boot (bit 0) and whether `paperpaw.ini` says `dual-boot = off` (bit 1), and `command.firmware_dual_boot` (1595) turns the check off (always confirmed by the host) or back to `auto`, writes `paperpaw.ini` and restarts (`docs/abi/device.md` 5.3), for the update plugin's interface-6 dual-boot notice and button.
