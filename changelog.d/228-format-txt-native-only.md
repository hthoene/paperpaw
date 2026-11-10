### Changed

- **Plain text books run on interface 7 only** (#228). The interface-6 text plugin is gone: a `.txt` or `.text` file opens through the native `format-txt` plugin, with the same paragraphs, encodings (UTF-8, Windows-1252, UTF-16 read from a converted copy) and Project Gutenberg titles, and the converted copy of a UTF-16 book is kept in the host's document cache instead of a folder of the plugin. It is no longer registered by the app; until native plugins are switched on in the shells, text files do not open as books.
