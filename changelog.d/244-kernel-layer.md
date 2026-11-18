### Changed

- **The kernel's screen stack holds its own kind of screen (#244, slice D5).** The stack holds the kernel-private `Layer`; interface-7 plugins' screens and the kernel's question dialogs are layers, and a screen of the old SDK `Screen` trait is held through an adapter until the last one is gone. Nothing a user sees changes.
