### Changed

- **Readwise Reader on interface 7, first slice** (#228). The set-up screen, its setting, actions and
  icons are declared for the native plugin interface, and the article ids, the books' start, the lists
  of articles fetched and read, and the copying of an article's text out of Readwise's answer are
  rewritten to refuse instead of aborting when memory runs short. Articles are fetched as before; the
  native plugin takes over once the host runs native plugins.
