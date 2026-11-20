### Changed

- **Less memory for every plugin** (#405). The kernel kept each built-in plugin in three of its tables (its instance, the directory the services ask, and the list of plugins built into the firmware); it is now one row, which the services and the file service read too. That is about 40 bytes less per plugin on the X4. Nothing else changes.
