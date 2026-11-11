### Fixed

- **A book opened while the native reader shows one opens** (#243). The reader opened the second book without giving back the first one's document, so the host refused it and the reader said the book could not be opened; everything after landed on that message (no bookmarks, no page turns). The same book now stays open where it was; another book opens after the reading screen below gives its document back, and that screen opens its book again when it is on top again. At the start, the book read last is not opened again over a book already on show.
- **Bookmarks, notes and the reading place are saved by the native reader** (#243). Its metadata lacked the `uses.library` feature, so the host refused every library call and the reader said memory may be full.
