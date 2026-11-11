### Added

- **Positions through `core.library` on the card** (#228). Op 14 `set_position` writes the book's position file, the SDK's own `[position]` form (so an interface 6 reader reads it), atomically, with the book's fingerprint kept; the host adds `insert` (when not 0) and `sync = 1` (a sync write that the user did not confirm), and op 13 `position` now reads both back, `from_sync` included.
