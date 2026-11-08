### Changed

- **Read-later Cloud and Joplin on interface 7, second slice** (#228). Read-later Cloud's list of
  articles and its answer about one article are read with the SDK's fallible JSON parser, so a
  refused allocation ends the reading with an error instead of aborting; its article lists and
  answers keep the read caps of interface 6 (64 KiB, 16 KiB, 512 KiB, 4 MiB), and its article
  copier is spent after a refusal, so the fetch ends. Joplin reads an item's properties from the last
  2,048 bytes of its file and a notes file's title from the first 1,024, as before, in fallible
  memory. Articles and notes are sent as before; the native plugins take over once the host runs
  native plugins.
