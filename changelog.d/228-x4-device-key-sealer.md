### Added

- **Secrets are sealed with a device key on the Xteink X4** (#411, #243). `core.secrets` keeps a plugin's secrets on the card sealed with a key that never leaves the chip's flash, so a copy of the card does not open them. Values an older version kept veiled are sealed on the next save of a secret (sealing needs the radio's noise for its nonce, so it cannot run at boot). The key is never written over data that is not the device's own, and the X4 Classic, whose `nvs` is the factory's, seals nothing until its key has a place of its own.
