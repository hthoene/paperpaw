### Fixed

- **Page turns on the X4 Pro no longer wait for the page count** (#518).
  While a book was still being counted, the page turned at a button's release
  was drawn only after a whole step of the count, a second or two later. The
  page is now drawn first and the count goes on after it, in shorter steps,
  so a press during a step waits half a second at most.
