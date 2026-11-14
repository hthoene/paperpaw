### Fixed

- **Removing a widget or a page forgets what it set, and "Reset to defaults" works for
  interface-7 plugins** (#257). The host refused `command.reset_setting`. It now resets one
  setting, what a placement set, a group, or the submitter's own settings, as
  `docs/abi/registry.md` 4.1 describes. The two resets that need the reader's consent (another
  plugin's settings, every setting) are refused with `ERR_UNSUPPORTED` until the host can ask.
