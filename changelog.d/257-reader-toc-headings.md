### Fixed

- **Reader: the table of contents of a book without one lists its chapters again** (#257). Where a book's own table has at most one entry, the native reader finds the headings in the book, shows how far it is, keeps them for the next time, and lists them, as Beta 3 did. A very long table keeps to a fixed amount of memory: the list then shows its first chapters and says that the rest is left out.
