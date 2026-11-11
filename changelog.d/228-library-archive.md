### Fixed

- **A library query leaves out the archive folder again** (#228). `core.library` reads the files plugin's `files.archive` setting when a query asks to leave the archive out (`lib_query.flags` bit 1) and hands it to the library host, as Beta 3's book lists did; a query that does not ask never reads the setting.
