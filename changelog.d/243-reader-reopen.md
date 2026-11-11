### Fixed

- **A book opened while the native reader shows one opens** (#243). The reader opened the second book without giving back the first one's document, so the host refused it and the reader said the book could not be opened; everything after landed on that message (no bookmarks, no page turns). The same book now stays open where it was; another book opens after the reading screen below gives its document back, and that screen opens its book again when it is on top again. At the start, the book read last is not opened again over a book already on show.
