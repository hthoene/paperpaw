### Added

- **Serving HTTP from the native SDK** (#228). `Net::serve` (`Server::new(token, port)` or
  `Server::cable(token)`, `max_body`), `Net::stop_serving` and `Net::respond` (`Response`: status,
  content type, up to 8 headers, an inline body or a file). The server's state and the requests it
  gets arrive as `event.server` and `event.served_request`. The SDK checks the port, the body cap,
  the status range, the sizes and counts, one of body or file, and a header's name (an HTTP token),
  value (no control bytes) and the host-owned names (`Host`, `Connection`, `Content-Length`,
  `Content-Type`); the host checks the rest (who may serve, how many servers, the request's owner).
