### Changed

- **The kernel's Bluetooth model is built only for devices that have Bluetooth** (#306). The `bluetooth` feature of `paperpaw-core` (on for the X4 Pro's `bluetooth` build, the desktop, Android and web shells) carries the radio model; the X4 and the X4 Classic, which have no Bluetooth, build without it and are 17–22 KB smaller. Plugins' Bluetooth types are unchanged; on such a device a Bluetooth command is refused as "off", as before.
