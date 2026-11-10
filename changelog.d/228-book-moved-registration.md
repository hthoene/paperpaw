### Changed

- A native plugin can register for `event.book_moved` and `event.book_removed`
  (68 and 69) as well as the other book events, so a plugin that subscribes to
  them (the reader) is not refused. Nothing is delivered until the library
  moves and removals raise them (`core-events.md` 3.3).
