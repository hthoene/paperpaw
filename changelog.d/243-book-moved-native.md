### Fixed

- Native plugins: `event.book_moved` is now delivered to subscribed plugins when the library moves a book (old and new path), so the reader's last book follows the move. A path over the record's 512 bytes drops the event and logs it once instead of telling a cut path.
