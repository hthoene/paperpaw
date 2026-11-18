### Changed

- **All notes on interface 7, library scan** (#228). Which books have notes is found through the
  library service: the catalog is passed a page at a time and each book's highlights and bookmarks
  are counted, keeping only the 200 books of the list however many books there are. A library that
  changes during the scan is read again; a reply that does not add up ends the scan with an error.
  The notes are listed as before; the native plugin takes over once the host runs native plugins.
