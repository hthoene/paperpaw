### Changed

- **The document and lookup services keep no room until they are used** (#248). The document table, its event queue and the lists of books and unfinished formats are reserved at the first document opened (the list of ended images grows as documents end, as it did), and the lookup service's table of pending asks is made by the first ask that is kept (or the first ready command); a device that never opens a document or waits for a provider holds none of it.
