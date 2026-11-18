### Added

- **TLS for `core.sockets`, core side** (#228). `connect` with the `tls` flag
  and pins, `start_tls` and `peer` now work on a shell that provides the TLS
  trait; without it they stay `ERR_UNSUPPORTED`. The device's one TLS session
  is a lease in the network pool that `core.net` and sockets take first come,
  first served, one holder at a time. A secure socket waits for it up to
  `timeout_ms` (`busy`), a secure socket idle for 5 s while another waits ends
  `session_needed`, `untrusted` and the `tls`, `name` and `expired` failures
  come from the shell's reports, and every end path gives the session and the
  lease back before the plugin is told.
