### Added

- **`font.acquire` takes `data:<path>`** (#244). A font pack or font file in the plugin's own data folder is opened when acquired (a pack is read where it is) and counts with the eight font handles; the SDK's `native::fonts::Font::acquire_data(host, path)` builds the key.
