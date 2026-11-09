### Added

- **`DirStorage::same_entry`** (#228): the desktop and Android directory storage answers by the file system's identity of the two entries (device and inode; the final path where std has no identity), so another case of a name, a hard link and a symlink are one entry. Paths it refuses and names that lead nowhere are `Some(false)`; a root that is gone is `None`.
