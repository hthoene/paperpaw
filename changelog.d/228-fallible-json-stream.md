### Added

- **Fallible JSON streaming** (#228). `paperpaw_json::stream::Parser::try_feed`,
  `try_finish` and `try_read_file` report refused memory as `OutOfMemory` instead of aborting,
  and never report a cut or partial event, so plugins reading catalogs on the X4 can stop
  cleanly. `feed`, `finish` and `read_file` report the same events as before.
