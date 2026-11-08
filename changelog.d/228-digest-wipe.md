### Changed

- **Digests wipe what they held of a password** (#228). `Md5`, `Sha1` and `Sha256` from
  `paperpaw-digest` (also `paperpaw_sdk::digest`) wipe their state when they go, also after
  `finish` and `hex`, and the block copies and message words of each step, so a password hashed
  for a sync key is not left in their memory.
