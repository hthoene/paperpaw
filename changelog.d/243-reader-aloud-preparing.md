### Fixed

- **Read aloud at the start reads the book reopened in the native reader again** (#243). *Read aloud* asked at the start of the device could reach the reader before the start itself, before the book read last was asked to open; it was dropped, so nothing was said. It is now kept while no book is on show and read for the book reopened. *Read aloud* asked while a book opened for the first time is still being prepared now starts as soon as it opens.
