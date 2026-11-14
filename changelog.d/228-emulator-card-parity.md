### Changed

- **The emulator's plugins from the card are offered what the device offers** (#228). A plugin file on the card is checked against the services and features the emulated device has wired, the same rule as the firmware's card path, so a plugin that needs the network, the card's files or the library loads here when it loads on the device, and is refused where it would be refused there.
