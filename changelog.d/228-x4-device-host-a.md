### Added

- **X4 `core.device` host, first part** (#228): `paperpaw_x4::device_host::X4Device` answers the device (op 1), the named readings (op 2), the plugins built into the image (op 4: the plugin id of each crate the image lists, as an update's check names them) and the physical keys (op 15). The other parts, and wiring it into the firmware, follow; the firmware does not offer `core.device` yet.
