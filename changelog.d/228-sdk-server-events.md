### Added

- **Server events in the native SDK** (#228). `Event::Server` (`ServerEvent`,
  `ServerState`: started with its address, failed or stopped with a reason) and
  `Event::ServedRequest` (`ServedRequest`: method, target and the body length when
  the request has a body), with `EventKind::{Server, ServedRequest}` and the
  `kinds::{Server, ServedRequest}` markers.
