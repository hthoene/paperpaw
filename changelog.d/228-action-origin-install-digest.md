### Added

- **Actions know who ran them** (#228). `Event::ActionRun` says `background` when a rule or
  another plugin ran the action rather than the reader (interface 6 `Env::reader`), so wallabag
  and OPDS can fetch at once for a press and as declared work for a rule, as in Beta 3.
- **The store can name the code's expected digest** (#228). `command.install_author` carries the
  `sha256` that `device.md` 5.2 describes; the host refuses an install whose code differs
  (reason 18). Interface 7 draft 35.
