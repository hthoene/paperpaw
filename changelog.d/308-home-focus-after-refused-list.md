### Fixed

- **Home asks the library again after a start without room for the books** (#308).
  A key press right after a start whose book list was refused for lack of
  memory is no focus step: the list is read again, so "Continue reading"
  shows and opens.
