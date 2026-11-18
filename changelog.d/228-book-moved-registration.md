### Changed

- A native plugin can register for `event.book_moved` and `event.book_removed`
  (68 and 69) as well as the other book events, so a plugin that subscribes to
  them (the reader) is not refused. Nothing is delivered until the library
  moves and removals raise them (`core-events.md` 3.3).
- A native plugin can register for `event.stack_changed` (81, a hint) too, so
  a plugin that closes what a covering screen hides (help) is not refused. The
  hint is raised when the stack path reports it (`ui.md` 5.1).
