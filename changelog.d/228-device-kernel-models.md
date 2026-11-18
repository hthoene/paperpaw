### Changed

- `core.device` answers Wi-Fi (op 8), Bluetooth (op 10), the bonded devices (op 11) and the power level and mode (op 12) from the kernel's own models on every device, so a plugin that reads right after another changed a radio sees the new state. Each plugin lists only the bonded devices given to it; the Bluetooth manager lists all of them.
