### Fixed

- **The kernel's placeholder and safe-mode screen are drawn whole on devices that draw in bands** (#243). The screen shown for a slot without a plugin, which is also all a safe-mode start shows on the X4, took the band being drawn for the screen: its frame and text were squeezed into the first band and the safe-mode text was repeated in each. It now lays itself out in the screen's room.
