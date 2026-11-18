### Added

- **`event.document_closed` in the native SDK** (#243). A plugin that opens documents
  subscribes with `EventKind::DocumentClosed` and gets
  `Event::DocumentClosed { now_ms, document, reason }` when the host ends one of its
  documents: the format stopped, the card went, the book was moved or removed, or the
  place went to a book to show (`DocClosedReason`). Before, it learnt this only from
  `Cancelled` on its next call.
