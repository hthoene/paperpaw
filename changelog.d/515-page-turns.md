### Fixed

- **Pages turn without delay again while a book's pages are counted** (#515).
  On the X4 Pro and X4 Classic a page turn could wait a second or two for the
  background count of the book's pages; a turn now waits for none of it, and
  the count goes on in the quiet time after the turn. Books opened after the
  first one are counted at the speed of these devices again, not a page a
  second. Background work of any plugin no longer runs between pressing a
  button and the action it makes.
