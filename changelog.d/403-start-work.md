### Changed

- **Faster start to the home screen** (#403). Each built-in plugin's declarations are now
  checked once when the plugin is taken in, not again at every later step of that
  same step. A menu row finds its icon and its submenu with one lookup instead of
  reading every icon and every menu row. On the X4 profile of the emulator, taking in
  the built-in plugins takes about 19 % fewer instructions and the first home frame
  about 26 % fewer. The memory used stays the same.
- The X4's start log splits the kernel's start into its parts (before the start event,
  the start event and its commands, settling).
