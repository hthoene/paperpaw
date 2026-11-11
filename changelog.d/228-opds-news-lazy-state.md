### Changed

- **The catalogs (OPDS) and news plugins keep their state until it is needed** (#248). Idle, each holds only a pointer; the state is made by the first screen, sync, fetch or widget event, so a device that never opens them keeps about 1.1 KiB more heap free.
