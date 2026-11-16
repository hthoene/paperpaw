### Changed

- **SDK: fifteen more modules show a plugin only what interface-7 code uses** (#244). Without the `kernel` feature `annotations`, `builtin`, `decorations`, `filters`, `help`, `install`, `library`, `logo`, `lookup`, `readers`, `reading`, `status`, `time`, `ui` and `widgets` are facades of 60 items in all (the calendar functions and time zones of `time`, the ids of `builtin`, `widgets::grid`'s geometry, `reading::WordBox`, `install::Package` and the like); the old widget tree, the catalog and the rest are the crate's own there. With the feature nothing changes. The check that builds every plugin's library alone now also fails where a build has the SDK's `kernel` feature on without being listed, and takes each crate's features from `cargo metadata`.
- **SDK: `native::events::PowerChange::level()` returns `native::device::PowerLevel`** (#244), the type
  `PowerInfo::level()` already gives. It returned a type behind the `kernel` feature, which a plugin
  could not name, so it could not tell "saving" from "offline". `tools/interface.py` now fails when a
  public signature of the SDK names a type its listing does not declare, beyond the eight known ones.
