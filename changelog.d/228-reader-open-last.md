### Fixed

- **The reader on interface 7 opens the last book at the start** (#228). With `reader.open-last` on, the
  book read last opens again when the device starts, if it is still on the card; the last book is
  remembered when a library book opens, as in Beta 3. Before, the native reader had no start handler and
  the start stopped at the home screen.
