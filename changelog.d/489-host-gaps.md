### Fixed

- **The X4 says how long a typed text may be** (#489). A question said
  24,576 bytes, but the X4 keeps only 2 KiB of texts, so a longer note was
  refused while it was typed and could never be sent. The X4 and the
  emulator of its profile now say 1,856 bytes, and the keyboard stops there.
- **An interrupted download no longer stays on the card** (#489). A power
  cut while a file was being downloaded (an update of several MB) left its
  temporary file behind for good. It is removed at the next start.
- **Messages from plugins show again when none of their screens is open**
  (#489). A message a plugin sent in answer to an event, such as the Plugin
  store's summary after the setup added plugins quietly, was dropped when
  none of the plugin's screens was open. It shows over whatever is open
  again, as in Beta 3; background work stays quiet.
