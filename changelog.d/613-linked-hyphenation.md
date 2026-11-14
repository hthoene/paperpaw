### Fixed
- The emulator serves hyphenation to the built-in plugins when no code plugins are trusted: the German pack on the card now changes how the tools drawer breaks its names, as on the device (the kernel takes a hyphenation sink from the shell without a code loader, `KernelBuilder::hyphenation_sink`).
