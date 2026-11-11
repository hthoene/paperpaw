### Added

- **`event.library_changed` reaches the plugins that subscribed** (#245). `core.library` tells them what changed (books, status, positions, annotations, fields); the changes raised between two passes arrive as one event, and the plugin that changed something hears it too. A plugin may register for it now (the native `files` plugin was refused at admission before).
