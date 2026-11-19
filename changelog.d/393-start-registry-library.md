### Changed

- **Home comes up sooner after a start** (#393). Drawing Home asked the plugin registry for its widgets and tool menus, and every row of a menu or a widget list read all the plugins' declarations again; it now reads them once for each question. Home's recent books ask the library only for the books opened before, not for a page of 32 books with a card read each.

### Added

- **`Show::Opened` for library queries** (#393). A query can list only the books opened at least once (being read or finished), as a list of the books read last needs them, without reading every other book's row. A host before it refuses the query with `Error::Invalid`.
