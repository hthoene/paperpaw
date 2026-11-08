### Changed

- **A lookup provider without an entry no longer ends the search** (#228). The
  broker of `core.lookup` turns a provider's "not found" into an empty entry
  with its name, so the providers after it are still asked; only running out
  of providers ends the asker's loop.
