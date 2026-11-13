### Fixed

- **A plugin's configured settings no longer cost heap through the whole start** (#248, #228). When a native plugin registers, the device defaults, the choices other plugins offer and the values of `settings.ini` for its settings are applied, which reads each such setting's description from the plugin. Those descriptions stayed on the heap until the end of the round, and at the start every plugin registers within one: about 0.36 KiB per configured line, up to about 23 KiB at the registration peak. They are let go at the end of each plugin's publication now.
