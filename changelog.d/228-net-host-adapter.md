### Added

- **Production `core.net` host for interface 7** (#228). The core now
  serves HTTP and the local server from the storage and the clock it is
  given: it checks the save target and the staged file against the files
  the card is using, keeps free room back for the system, writes a saved
  answer to a staged file and renames it only when whole, and cleans up
  `.received/` when a plugin stops. A shell wires it with
  `HostWiring::net` and only supplies the transport (`ShellHttp`) and the
  network and power state (`ShellNetwork`).
