### Changed

- For developers: what the kernel's own parts, the shells and the tools register with is the kernel's own type now (`paperpaw_core::Items`); the SDK no longer has `Registrar` or the old `Plugin` trait. Nothing a device does changes, and plugin files built before load as before (the interface fingerprint is the same).
