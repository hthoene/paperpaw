### Added

- **Reader: the stage `reader.notes-exported`** (#228). After the reader wrote a book's notes as Markdown it runs this stage (the file's root, path and title) for notes apps and syncs to take the file further; it replaces the event `core.notes-exported`.
