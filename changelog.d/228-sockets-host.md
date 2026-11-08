### Added

- **`core.sockets` host, first slice** (#228). The core now keeps the socket
  table of the service: plain TCP sockets with `connect`, `send`, `read` (bytes,
  also exact), `data` and `close`, the event `event.socket`, two sockets per
  plugin and per X4, and the buffers (send queue and read part) charged to the
  same network pool as `core.net`. Sockets end with the screen or work run they
  were opened in, after the idle time, when the network goes and when the
  plugin stops. TLS, reads to a file and UDP broadcast answer
  `ERR_UNSUPPORTED` until their slices land; a shell still has to wire the
  transport.

- **Reads to a file** (`read` mode 1): the bytes go to the staged file of the
  target and are renamed over it only when all `len` bytes arrived; then the
  plugin is told `saved`. The target and its staged file are in use meanwhile,
  the card rules and the free-space reserve apply, and anything that ends the
  socket first (the peer, a full card, the idle time, `close`, the screen, the
  run, the plugin stopping) removes the staged file and leaves the old file.
  An `exact` read is all or nothing: the peer closing early ends the socket
  with `closed_early`. Core now combines the network demand of `core.net` and
  the sockets, so the shell hears one edge per change.
