### Fixed

- **The built-in plugins that declare settings keep them as a table in the program too** (#228, #248). Keyboard, lock, help, OPDS, power, Bluetooth, menu, settings, file manager and aids take their settings from a table their build script makes of the same declarations (as the widget pack does), instead of a copy on the heap for each: about 7 KiB less heap on the X4 profile in the emulator.
