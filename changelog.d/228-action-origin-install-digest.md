### Added

- **Actions know who ran them** (#228). `Event::ActionRun` says `background` when a rule or
  another plugin ran the action rather than the reader (interface 6 `Env::reader`), so wallabag
  and OPDS can fetch at once for a press and as declared work for a rule, as in Beta 3.
- **The store can name the code's expected digest** (#228). `command.install_author` carries the
  `sha256` that `device.md` 5.2 describes, for the host's install check (an install whose code
  differs ends with reason 18 once the install handler lands). Interface 7 draft 34.

### Changed

- **Shorter rotation chains per install** (#228). A store install carries at most 6 key
  rotations (Beta 3: 8), so the install, its author and its rotations fit one 8-record batch; a
  catalog entry with a longer chain ends with reason 6 (key changed). At most 8 revocations go in
  one batch (was 16), more go in later batches.
