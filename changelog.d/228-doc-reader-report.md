### Added

- **The reader report** (#228, interface 7 draft 37). `core.document` op 22 `report` (`service.doc_report`) lets the reading app tell the host where the user reads: position, progress, title, chapter, page counters and "finished". The host raises the book events from it (`event.book_opened`, `event.page_turned`, `event.book_finished`, `event.book_closed`), checks every field and stamps the time, path and reason itself, so no plugin raises them. The events gain the optional `chapter_left`, the pages left in the chapter, for the reading widget's "pages left in the chapter" line.
