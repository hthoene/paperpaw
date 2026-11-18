### Fixed

- **A pulled SD card or a revoked share no longer makes every book look
  deleted** (#228). A folder volume whose mount point is gone (the desktop
  emulator's card folder, Android's shared storage and SD cards) now answers
  "card gone" on every operation, where it answered "not found". The library
  reports a read error for its books, not "missing", and a write no longer
  makes the mount point again on the wrong disk.
