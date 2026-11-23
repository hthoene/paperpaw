### Changed

- **Smaller firmware images** (#503). The kernel's registry service sorts with the firmware's one shared sort instead of pulling in a second one, and the SDK compiles the checks and the allocation that make a native plugin once instead of once per plugin. Measured on release-like local builds: X4 5,680 bytes smaller, X4 Pro 6,768 bytes smaller.
