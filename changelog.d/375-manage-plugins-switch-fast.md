### Fixed

- **Switching a plugin on or off in Manage plugins no longer freezes the X4** (#375).
  Settings built its main page twice behind the plugin list, once for every
  plugin switched, which took about 16 seconds; with more plugins on the card the
  watchdog restarted the device. Settings now builds a page only when it is
  shown, so a switch takes about as long as opening the list. Long host work for
  a plugin also keeps the watchdog fed as a safety net.
