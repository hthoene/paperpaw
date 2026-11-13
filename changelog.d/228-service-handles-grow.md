### Changed

- **The host's handle table grows as handles are opened** (#248). It grew by eight slots at a time, so the first service a plugin opened held room for seven more handles (about 1 KiB on the emulator); it now grows by one, and a growth without room is refused with no memory while the handles already open stay.
