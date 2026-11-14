### Fixed

- **A number changed with the buttons shows its value at once again** (#257). The host refused
  the preview commands of interface-7 plugins (`command.preview_setting`, `command.preview_end`),
  so the Settings screen showed the new value only once it was chosen. They are now accepted as
  `docs/abi/registry.md` 4.4 describes; a preview also ends when the device goes to sleep.
