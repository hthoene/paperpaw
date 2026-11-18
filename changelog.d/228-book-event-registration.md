### Changed

- A native plugin can register for `event.book_opened`, `event.book_closed` and
  `event.book_finished` (65 to 67), so Rules and Setup load. Nothing is
  delivered until the reader port produces them (`core-events.md` 3.1).
