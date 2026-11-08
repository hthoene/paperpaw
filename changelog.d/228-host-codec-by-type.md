### Changed

- **The host reads and writes records by type** (#228). Service, event, input
  and library ops decode the record types they expect instead of the
  enum-wide `Record` decoder, and encode with the typed encoders, so the X4
  image no longer links `record_decode`, `Record::write`, `Record::body_len`
  or `Record::tag`. Statuses are those of the enum path; the one intended
  change is that `events::decode` gives `ERR_UNSUPPORTED` for a non-event
  record with a malformed body. In release builds a service reply is checked
  by the framing of every row and its typed end row (the rows of `core.lookup`,
  whose provider is a plugin, by their types); debug and test builds read
  every row by its type.
