### Changed

- **Wallabag on interface 7, second slice** (#228). The list of unread articles and an article's
  archived flag are read with the SDK's fallible JSON parser, so a refused allocation ends the
  reading with an error instead of aborting; the lists of articles fetched and read keep the read
  caps of interface 6 (64 KiB and 16 KiB) and say so when a file is over them. Articles are fetched
  as before; the native plugin takes over once the host runs native plugins.
