### Fixed

- **The sleep screen in cover mode shows the cover of the book read last, also
  right after a restart** (#346). The screen asked the library for the books in
  falling "recently opened" order, which puts the books never opened first, so
  it found no book read and showed the cat. The host noted a cover as made per
  book, not per size, so the screen-sized cover was decoded inside the sleep
  screen's call where only the list's was in the cache. And after a start
  straight into the reader the host answered "not now" for the book's cover
  while the device went to sleep; the book being read now gets its list-sized
  cover at once (from the cache, or decoded where the heap has the room).
