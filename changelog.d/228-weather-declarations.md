### Changed

- **Weather on interface 7, first slice** (#228). The settings group, the place and its screen, the
  units, how often to fetch, the actions and the icons are declared for the native plugin
  interface, and the numbers, dates, coordinates, addresses, the weather kept in its file (read to
  4 KiB as before), the conditions, the units and the timing are rewritten to refuse instead of
  aborting when memory runs short. The weather is fetched and shown as before; the native plugin
  takes over once the host runs native plugins.
