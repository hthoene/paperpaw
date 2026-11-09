### Changed

- **Wi-Fi: what the plugin does with the radio's events on interface 7** (#228). Scans, joins, a
  network wanted or idle, the typed password, the screen shown, charging and the two timers are
  decided without the host (the five radio scenarios of interface 6 give the same results) and
  refuse cleanly when memory is short; the radio's events 92-95 are subscribed. The timers and the
  callbacks follow.
