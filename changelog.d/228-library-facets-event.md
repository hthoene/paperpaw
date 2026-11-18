### Added

- The native SDK reads library facets (authors, series, tags, first letters,
  folders) through `Library::facets` and receives the library-changed hint as
  the typed `Event::LibraryChanged` and `EventKind::LibraryChanged` (#228).
