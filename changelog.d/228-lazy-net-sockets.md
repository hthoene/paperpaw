### Changed

- **Network services made at their first call** (#248). `core.net` and `core.sockets` are wired
  at start but their devices are made when a plugin first calls them, fallibly: a first call
  without room is `ERR_NO_MEMORY` and the next one tries again. The shell's reports before that
  answer as an empty device does. A device whose plugins never use the network keeps about
  0.7 KiB more free.
