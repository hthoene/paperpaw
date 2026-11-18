### Added

- **The library's state names the progress of a first walk** (#228). While the
  catalog is made where there was none, the kernel reads how far the walk is
  (`Walk::progress_ppm`) for `core.library` op 1; its books are those found so
  far, and the share is only a rough bar. `catalog::is_growing` tells whether
  such a walk is under way.
