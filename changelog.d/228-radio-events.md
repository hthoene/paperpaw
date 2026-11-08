### Added

- The native SDK receives the radio events as typed `Event::WifiScanned`,
  `Event::WifiDone` (with `WifiFailure`), `Event::NetworkWanted` and
  `Event::NetworkIdle`, with matching `EventKind` subscriptions (#228).
