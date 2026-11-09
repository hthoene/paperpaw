### Added

- **`DirStorage::same_entry`** (#228): the desktop and Android directory storage says two paths are the same entry only where they are the same name (another case of it, where the file system ignores case) in the same folder, so a case-only rename is told from a move onto another file. Nothing is followed at the last name: a hard link, and a symlink and its target, are two entries. Paths it refuses and names that lead nowhere are `Some(false)`; a gone root is `None`; Windows answers `None` until that path has been run there.
