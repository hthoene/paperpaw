### Added

- **Secrets are sealed with a device key on the Xteink X4** (#411, #243). `core.secrets` keeps a plugin's secrets on the card sealed with a key that never leaves the chip's flash, so a copy of the card does not open them. Values an older version kept veiled are sealed once at start; a failure there is logged and never blocks the start.
