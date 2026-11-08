### Added

- **Mail on interface 7, second slice** (#228). The reader that turns an HTML mail into text runs without panics and reports out-of-memory instead of aborting. It makes the same text as before, but a mail can no longer crash it with a non-ASCII character inside a style or script block, a long comment no longer hides the rest of the mail, and a mail's text is held to 24 KiB.
