### Fixed
- The host's cache of a document (`.paperpaw/cache/<book key>/`) is removed, a bounded piece at a time, with the old key of a book that is moved, replaced or deleted (its stamp names the book key, so it is never renamed); the new key's is made at the next open. A cache too big for one step holds the library move's phase until it is gone.
