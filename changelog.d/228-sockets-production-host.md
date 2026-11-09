### Added

- **`core.sockets` can be wired by a shell** (#228). `HostWiring::sockets` takes
  `SockInputs` (the card, a `SockTransport` for TCP and UDP, optionally a
  `SockTls`, the same `ShellNetwork` as `core.net`, and `SockLimits`) and gives
  back `SockReports`, the calls the shell's loop makes for each outcome
  (connected, bytes, closed, failed, answers, `secured`, `untrusted`, TLS
  failures, time passed, network lost). Core does the rest: the limits, the one
  network pool shared with `core.net`, reads to a file, the events. A shell
  without TLS leaves `connect` with `tls`, `start_tls` and `peer`
  `ERR_UNSUPPORTED`, so nothing a plugin asked to be secure is ever sent in the
  clear.
- **One TLS session lease for HTTPS and sockets.** A device has 1 to 4 TLS
  sessions (the X4: 1, `NetLimits::tls_sessions` and `SockLimits::tls_sessions`).
  An `https` request and a secure socket take one in the order they ask and
  give it back on every way they end; a request or socket at the head of the
  queue that ends leaves it, so nobody waits behind it.

### Changed

- **Saved files and reads to a file lose nothing at a failed commit.** One
  path (`save_files`) serves `core.net` and `core.sockets`: the staged
  `.pp-staged` file is flushed before it is renamed over the target, and a
  rename that fails after the old file is gone leaves the staged file in place
  as the only copy (the next save to that path replaces it). A read to a file is
  exclusive: no other writer, and no second read or request of the same plugin,
  may hold the target or its staged file.
- **A plugin's own storage writes cannot meet the host's temporary file.**
  `write`, `append`, `rename` and `remove` of a file that `core.net` or
  `core.sockets` holds (the target or its `.pp-staged` file) answer `ERR_BUSY`.
  Names that FAT takes for one are one in this check beyond ASCII (`É` and `é`).
