### Fixed

- **Settings open quickly again** (#422). Opening the settings (from the home screen or the menu) took about 7.5 seconds on the X3 and as long on the other devices: for every setting row, and again for every page of rows, the kernel read all plugins' declarations anew. What a settings list needs from them (which settings have a device need or a declared editor, which settings others depend on, how many rows a list has) is now read once and kept until a plugin changes, and the rows look up their plugin's records in order. Nothing else changes in what the settings show.
