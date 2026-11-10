### Changed

- A native plugin can register for `event.book_moved` and `event.book_removed`
  (68 and 69) as well, so the reader's whole event registration is admitted.
  Nothing is delivered until the library moves and removals raise them
  (`core-events.md` 3.1).
