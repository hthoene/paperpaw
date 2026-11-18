### Changed

- Rules 1.0.8: the interface-7 native plugin (the engine on the host's events, the list with pause,
  conditions and removal, adding a rule step by step); `rules.txt` kept byte for byte, lines this
  version cannot read are kept. Time of day and "every so many minutes" follow the minute tick; the
  interface-6 plugin, its app wiring and its store file are removed (#228).
- Hints 1.0.4: the interface-7 native plugin (hints over a screen, show again, the settings) on
  `paperpaw-ui-kit`'s new `Dialog`; the button-hint labels while the hints are open wait for a native
  hint-row API; the interface-6 plugin and its app wiring are removed (#228).
