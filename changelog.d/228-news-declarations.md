### Changed

- **News on interface 7, first slice** (#228). The setting for the articles a feed keeps, the list of
  feeds, its actions, the place in the tools and the icons are declared for the native plugin
  interface, and the feeds kept, the headlines, the feeds' dates and the text helpers are rewritten
  to refuse instead of aborting when memory runs short, with the read caps of before (64 KiB feeds,
  32 KiB headlines). Feeds are fetched as before; the native plugin takes over once the host runs
  native plugins.
