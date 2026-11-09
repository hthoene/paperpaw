### Added

- **X4 `core.device` host, second part** (#228): `X4Device` also answers the running firmware (op 3: version, project, chip, room, undone; the install state is the core's), what code plugins stand on (op 5), memory (op 7, one real probe per 250 ms, cached answers say so) and the plugins a start could not install (op 14). Not wired into the firmware yet.
