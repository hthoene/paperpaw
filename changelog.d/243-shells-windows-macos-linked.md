### Changed

- **Windows and macOS emulators host the first-party native plugins** (#243, Decision 172). The desktop emulator on Windows (x86-64) and macOS (x86-64 and ARM) now registers the linked interface-7 plugins as it does on Linux: the host profile is the declared one of the build's target, and the read-only data the kernel checks them against is found with `VirtualQuery` on Windows and `mach_vm_region` on macOS. The Linux-only developer loader for plugin libraries is unchanged.
