### Added

- **The kernel's firmware request table, queue and pump** (#228). Firmware installs and checks wait in a device-wide queue of at most four requests (running one included) that is accepted whole or refused with nothing changed (`ERR_FULL`, `ERR_INVALID`, `ERR_NO_MEMORY`), run one bounded device step at a time, keep their outcomes for `install_done` and op 13, and end with reason 20 when the host stops them. The firmware commands, the events and the confirmation screen follow.
