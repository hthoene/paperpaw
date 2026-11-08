### Changed

- **Digests leave no trace of what they hashed** (#228). `Md5`, `Sha1` and `Sha256` from
  `paperpaw-digest` (also `paperpaw_sdk::digest`) wipe their state when they go, also after
  `finish` and `hex`, so a password hashed for a sync key is not left in memory.
