### Added

- **Asynchronous lookups in the native SDK** (#228). A lookup provider that
  answered `pending` calls `CommandSender::lookup_ready(text, language)` when
  its result is kept; an asker subscribed to `EventKind::LookupReady` gets
  `Event::LookupReady { now_ms, provider }` and asks again.
