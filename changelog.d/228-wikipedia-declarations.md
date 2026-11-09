### Changed

- **The Wikipedia lookup on interface 7, first slice** (#228). The summary screen is declared (the online
  provider is declared too, once the host lists providers) for the native plugin interface, and the addresses and the reading of the
  summary are rewritten to refuse instead of aborting when memory runs short. The lookup works as
  before; the native plugin takes over once the host runs native plugins.
