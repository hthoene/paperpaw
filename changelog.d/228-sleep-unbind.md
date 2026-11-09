### Changed

- **`sleep` binds `core.sleep` only when `sleep.show` changes** (#228, Kernel's ruling). It no longer binds or unbinds at start, so a screen the reader bound by hand stays across restarts; a change to one of its own modes unbinds only where the slot is bound to another plugin's sleep mode.
