### Added

- **The reader reports its place with `Documents::report`** (#228). The reader tells the host where the user is (core.document op 22) and the host raises the book events; `ReadingView::chapter_left()` reads the pages left in the chapter from the page and book events (interface 7, draft 37).
