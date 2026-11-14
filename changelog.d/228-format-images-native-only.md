### Changed

- **Pictures as books run on interface 7 only** (#228). The interface-6 pictures plugin is gone: a JPEG, PNG or GIF opens as a book through the native `format-images` plugin, with the pictures of its folder after it as pages, and what is decoded is kept in the host's document cache instead of a folder of the plugin. It is no longer built into the app or offered as an interface-6 plugin file; until native plugins are switched on in the shells, pictures do not open as books.
