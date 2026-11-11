### Added

- **Native plugins can read aloud** (#243). `command.speak` and `command.stop_speaking` run on the same speech the shells give interface-6 plugins, and `event.spoken` tells the plugin that asked, and no other, how its text ended: spoken, replaced or stopped, no voice for the language, or no speech on this device (a device without speech says so at once). A plugin stops its own speech only.
