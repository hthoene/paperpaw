### Changed

- **Joplin on interface 7, first slice** (#228). The set-up screen, its settings, actions and icons are
  declared for the native plugin interface, and the sync targets' addresses, Joplin's ids and items
  (notebook and note properties) and the reading of a book's exported notes are rewritten to refuse
  instead of aborting when memory runs short. Notes are sent as before; the native plugin takes over
  once the host runs native plugins.
