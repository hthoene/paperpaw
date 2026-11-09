### Changed

- **News as an interface-7 plugin file** (#228). The news plugin is built as a native plugin file for the X4 (81,412 bytes), so the packer and the flash gate see it. The page of a feed is read through a buffer from the heap instead of one on the stack (the X4 refuses a frame of 2 KiB or more). The plugin has no list screen yet and still runs as before until the native plugin takes over.
