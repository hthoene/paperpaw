### Changed

- **Mobipocket books have an interface-7 entry** (#228). `format-mobi` declares its format (`mobi`, `azw`, `azw3`, with a sniff and the first-open conversion) and answers the host's `core.format` calls from a native entry, with the same blocks as before. The interface-6 form stays until the host activates native plugins.
