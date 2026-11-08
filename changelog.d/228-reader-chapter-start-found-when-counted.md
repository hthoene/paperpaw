### Fixed

- **The status line no longer reads the page index while it draws** (#228). The index remembers only its last answer, and the reader asked it for the chapter's start and end after the page itself, so the next draw looked the page up again, from the file. What the status lines ask of the index (the chapter's start and end, the page) is now found when the page is turned or the count ends, the page's own last.
