### Added

- **The emulator serves for native plugins** (#228). A plugin that serves a page through `core.net` (the transfer page, the phone page of the flash cards) now listens on a real port of the computer, on the loopback address, where it was refused before; uploads are written to the card as they come, answers and files are sent back, and the plugin's own limits apply. CI can now test serving.
