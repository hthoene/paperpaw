### Changed

- **Drawing a full frame takes fewer passes on the X4** (#267). Where the heap has room for it, the frame is drawn in two bands instead of five, which cuts the host calls of Home's return from a book by about a third. The shell's panel writes are unchanged.
