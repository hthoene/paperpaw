### Changed

- **Weather on interface 7, second slice** (#228). The forecast and the places a search finds are
  read with the SDK's fallible JSON parser, so a refused allocation ends the reading with an error
  instead of aborting, and the weekday of a date near the ends of the range no longer overflows.
  The weather is fetched and shown as before; the native plugin takes over once the host runs
  native plugins.
