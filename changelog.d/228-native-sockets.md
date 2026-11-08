### Added

- **Sockets in the native SDK** (#228). `native::sockets` wraps `core.sockets`
  major 1: `Sockets::{open, release, connect, send, read_bytes, read_file,
  data, start_tls, peer, close, broadcast}`, the `Connect`, `Tls`, `Pin` and
  `Broadcast` builders (checked as the host checks them, before any call), a
  `Reader` that keeps surplus bytes and reads lines as UTF-8 (and makes
  `start_tls` refuse with `State` while it holds any), an `Outbox` that queues
  a long message piece by piece on `Sent`, and the `Socket` and `Broadcast`
  events (`EventKind::Socket`, `EventKind::Broadcast`, `Event::Socket`,
  `Event::Broadcast`). Borrowed buffers, no allocation, no socket data in
  `Debug`.
