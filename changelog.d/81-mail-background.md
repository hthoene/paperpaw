### Added

- Mail (#81): an optional background check (settings "Check for new mail in the background", off by default; every 15, 30 or 60 minutes). It checks the accounts one connection at a time while the reader is awake and online, and the Home widget shows the unread count. A mail's EPUB, PDF, MOBI, AZW3, FB2, TXT, DOCX or CBZ attachment opens as a book from the mail ("Open book"). `tools/pps/mail/` holds an end-to-end script and a fake IMAP/SMTP server for the emulator.

### Fixed

- Mail's unread-count Home widget drew nothing.
- The desktop emulator and its card folder: listing a file, or going below one, answers as on the card (the wrong kind of path), so the library takes an imported file there.
