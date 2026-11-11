### Fixed

- **A configuration value for a native plugin's setting is applied** (#228). The kernel reads the configuration file (and the shell's `--set` values) when it is built, before a native plugin is hosted, so a value for a setting that plugin declares was warned about as "unknown setting" and never set. Such values are kept and set as the plugin is published; the warning is taken back, and a setting no plugin declares is still warned about.
