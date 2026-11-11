### Changed

- **The desktop emulator offers `uses.wifi` to linked native plugins** (#228). On a profile whose device has Wi-Fi (the X4), the pretend radio answers the radio commands, so the `wifi`, `transfer` and `diagnostics` plugins are admitted; profiles without a radio still refuse a plugin that needs it.
