### Added

- **X4 `core.device` host, fourth part** (#228): `X4Device` also answers the Bluetooth radio (op 10) and the bonded devices (op 11: at most 16, in the firmware's order) in builds that have a radio; builds without one answer neither. Not wired into the firmware yet.
