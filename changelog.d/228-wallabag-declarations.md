### Changed

- **Wallabag on interface 7, first slice** (#228). The set-up screen, its settings, actions and icons
  are declared for the native plugin interface, and the article ids, the lists of articles fetched
  and read, and the test for an article read to the end are rewritten to refuse instead of aborting
  when memory runs short. Articles are fetched as before; the native plugin takes over once the host
  runs native plugins.
