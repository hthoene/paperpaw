### Added

- **`event.screen_hidden`** (#248, interface 7 draft 37). A native screen is told when a full screen opens above it, so it can let go of what it keeps only to draw itself and build it again on its next `screen_shown`. Plugins that do not read it lose nothing.
