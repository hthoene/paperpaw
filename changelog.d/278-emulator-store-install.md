### Added

- Linux emulator (`--trust-code`): the store installs code plugins as real downloads, like a device does. The emulator has its own catalogue name, `emulator-linux-x86_64`, and a plugin's library (`code-host.so`) carries its author's signature after it (`paperpaw-sign sign-plugin` signs one); it is checked, asked about and installed into the plugin's folder, so its bundled pictures load from there. Code plugins built into the emulator stay as they were.
