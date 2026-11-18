### Added

- Code plugins: the check that refuses a host plugin library with a way to run code while it loads now also reads macOS (Mach-O) and Windows (PE) libraries, not only Linux ones (#233). A Windows library must have no entry point at all; TLS callbacks, delay-load imports and libraries outside the system's and the C runtime are refused, as are initialisers, Objective-C `+load` and libraries besides `libSystem` on macOS. The web emulator loads no code plugins, so it needs no such check.
