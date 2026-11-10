### Fixed
- The device's setting defaults (e.g. `keyboard.size` of the X4 and X4 Pro) now reach the settings a native plugin declares: they were dropped with an "unknown setting" warning because the kernel is built before the plugin is hosted. A default for a setting nobody has declared yet is kept and set as the plugin is published; a value the user set still wins.
