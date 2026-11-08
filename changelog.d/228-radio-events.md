### Added

- The native SDK receives the radio, firmware and install events as typed
  `Event::WifiScanned`, `Event::WifiDone` (with `WifiFailure`),
  `Event::NetworkWanted`, `Event::NetworkIdle`, `Event::FirmwareChanged` and
  `Event::InstallDone`, with matching `EventKind` subscriptions (#228).
