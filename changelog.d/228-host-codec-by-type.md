### Changed

- **The host reads and writes records by type** (#228). Service, event, input
  and library ops decode the record types they expect instead of the
  enum-wide `Record` decoder, and encode with the typed encoders, so the X4
  image no longer links `record_decode`, `Record::write`, `Record::body_len`
  or `Record::tag`. Statuses are those of the enum path; the intended
  changes are in `events::decode`: a non-event record with a malformed body
  gives `ERR_UNSUPPORTED`, and a batch with a count above 8 gives
  `ERR_INVALID` (it gave `ERR_NO_SPACE`). In release builds a service reply is checked
  by the framing of every row and its typed end row (the rows of `core.lookup`,
  whose provider is a plugin, by their types); debug and test builds read
  every row by its type.
