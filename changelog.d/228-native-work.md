### Added

- **Deferred and background work in the native SDK** (#228).
  `native::work::Work` declares, cancels and lists a plugin's work on
  `core.work`: `WorkSpec` says when it comes due (after, within, every, an
  exact time or a moment such as going online or charging) and whether it needs
  the network or only runs when that is cheap. `retry_delay_ms` gives the
  growing wait after a failure. Everything is checked before the host call and
  nothing is allocated. Receiving the due, refused and allowed events follows.
