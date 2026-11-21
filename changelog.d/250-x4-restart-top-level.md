### Fixed

- **Less X4 stack when the firmware restarts** (#250). A restart (from the menu or the kernel's command) is carried out at the top of the main loop, like sleep and power-off, so the kernel's stop and the events plugins hear on it no longer run deep inside the loop's effect handling. Behaviour is the same.
