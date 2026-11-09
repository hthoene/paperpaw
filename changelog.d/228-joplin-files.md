### Changed

- **Joplin on interface 7, third slice** (#228). The files the Joplin plugin keeps (the notes that wait, those sent, the notebook), the note it writes from a book's notes (copied in pieces, with its MD5), the reads of the sync target's files and the check for a lock held while a client upgrades the target are now written for interface 7 too, with every allocation fallible and the list of notes that wait read at most 32 at a time. The plugin still runs as before until the native plugin takes over.
