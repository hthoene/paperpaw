### Changed

- **The cover job waits while the library list is open** (#228, #248). The background conversion of a DOCX or MOBI book for its cover takes no conversion step while a screen of the files plugin is on the stack, and for 2 s after, so its memory peak no longer comes beside the list (a book that is ready is not held). The time it waits does not count toward the book's 10 minutes.
