### Fixed

- **A saved page turn on the X4 writes one block, not twelve** (#308). The
  storage wrappers of the X4 (its trace), the desktop trace, the Android
  volumes and a mounted storage did not pass an in-place write on, so the
  position file's new two-slot save (#305) fell back to writing the whole file
  anew on every save.
- **Held Back in the keyboard finishes and closes it** (#308). The action of a
  native plugin could not stage `close` (`ERR_UNSUPPORTED`, the warning "keyboard:
  call 1 ended with -1"), so the keyboard stayed open after its answer. An action
  has the screen stack, as a screen's callback has. The warning "native redraw
  dropped: its screen is covered" is gone: a redraw for a screen that is covered
  is dropped as before, it is no fault.
