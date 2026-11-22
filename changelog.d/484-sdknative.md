### Fixed

- **A long setting text is encoded for a preview** (#484): `settings::encode_value`
  refused any text over about 50 bytes with `NoSpace`, however large the caller's
  buffer; it now encodes every value that fits the buffer.
- **A screen's last word on the row of button hints counts** (#484):
  `ScreenDecl::hints_hidden()` followed by `.hints(..)` (or the other way round)
  declared both, which the host refuses; the later call now replaces the earlier.
- **`show_again` refuses a time under a minute** (#484): as documented,
  `CommandSender::show_again` now returns `Invalid` for `after_ms` (or a non-zero
  `every_ms`) under `SHOW_AGAIN_MIN_MS`; `after_ms` 0 no longer quietly cancels
  the request that waits.
- **A malformed `document_changed` event is refused** (#484): an event naming
  document 0 or a progress over 1,000,000 now decodes as `Invalid`, as
  `document_closed` and the book events already did.
- **A too-long path is no longer "card full"** (#484): `DataStorage` reported a
  path over its length cap as `StorageError::Full`; it is now `InvalidPath`.
