### Added

- **Widgets and status lines of other plugins** (#228). Screens can now make and drop widget instances and ask for the values and drawn lines of other plugins' status items through the `core.ui` service. Private ones stay hidden behind a lock screen, and a plugin never calls itself back. The X4 answers status requests as unsupported until its stack check passes.
