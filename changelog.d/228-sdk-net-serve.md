### Added

- **Serving HTTP from the native SDK** (#228). `Net::serve` (`Server::new(token, port)` or
  `Server::cable(token)`, `max_body`), `Net::stop_serving` and `Net::respond` (`Response`: status,
  content type, up to 8 headers, an inline body or a file). The server's state and the requests it
  gets arrive as `event.server` and `event.served_request`; every limit of `network.md` 5 is checked
  before the host is called.
