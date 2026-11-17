### Fixed

- **A saved page turn on the X4 writes one block, not twelve** (#308). The
  storage wrappers of the X4 (its trace), the desktop trace, the Android
  volumes and a mounted storage did not pass an in-place write on, so the
  position file's new two-slot save (#305) fell back to writing the whole file
  anew on every save.
