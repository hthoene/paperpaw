### Added

- **Command stage for native screens** (#243, H1). The input and event callbacks of a native
  screen now get a command stage in production: what they submit (open, close, back_to, notify,
  settings, item states, labels, widget changes) waits in a bounded screen queue (4 batches) and
  the kernel applies it between callbacks with the same code the tests use. A callback that fails
  applies nothing. `command.notify` is staged (bit 0 `announce`, other bits `ERR_INVALID`); an
  open that names a hand-over stays refused until the hand-over store is wired.
