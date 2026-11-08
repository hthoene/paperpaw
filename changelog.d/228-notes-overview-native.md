### Changed

- **All notes on interface 7, native screens** (#228). The list of books with notes and a book's
  notes have a native form, read through the library service, with the same texts. A book's
  notes are shown 100 to a page, with *Next notes* and *Previous notes* rows. The export to
  `My Clippings.txt` works through the library and files services (note texts are read whole up to
  8 KiB each, and times are in UTC until the time zone is available). The opening of a note at its
  place waits for the reader; that row says it is not available. The native plugin takes over once
  the host runs native plugins.
