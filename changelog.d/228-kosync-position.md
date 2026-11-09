### Changed

- **Progress sync names the book and the place through the library and document services** (#228). The book's fingerprint comes from `core.library`, the XPointer of a place and the place of an XPointer from `core.document`; where the device does not offer them the sync still does nothing, as before. The account's key is also zeroed where an allocation is refused while a request body or a file is made.
